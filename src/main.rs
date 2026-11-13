mod bind;
mod cookies;
mod db;
mod logging;
mod models;
mod notify;
mod scheduler;
mod security;
mod trackers;
mod tray;
mod web;

use anyhow::{Result, bail};
use chrono::Utc;
use clap::{Parser, Subcommand};
use db::Database;
use logging::{BroadcastLayer, LogBroadcaster};
use models::{ItemType, NotificationItem, UiEvent, UserRole};
use reqwest::Client;
use security::{
    AuthRateLimiter, generate_token, hash_password, sha256_hex, validate_password,
    validate_username,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use web::AppState;

#[derive(Parser)]
#[command(name = "ptn", version, about = "PTNotifier: Private Tracker Notification & PM Monitor")]
struct Cli {
    /// Path to SQLite database file (defaults to the OS user data directory)
    #[arg(long, global = true, env = "PTN_DB")]
    db: Option<PathBuf>,

    /// Log level filter (error, warn, info, debug, trace)
    #[arg(long, default_value = "info", global = true, env = "PTN_LOG_LEVEL")]
    log_level: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the background scheduler and embedded WebUI server (default)
    Serve {
        /// Address to bind the WebUI & API server (default 127.0.0.1:8085, or the address
        /// saved in the WebUI). When set, the WebUI cannot change it.
        #[arg(long, env = "PTN_BIND", value_parser = parse_bind_arg)]
        bind: Option<SocketAddr>,
        /// Run with a system tray icon (Windows, macOS, or Linux with `--features tray`)
        #[arg(long)]
        tray: bool,
        /// Trust X-Forwarded-For / X-Forwarded-Proto / X-Forwarded-Host headers from a reverse proxy
        #[arg(long, env = "PTN_TRUST_PROXY")]
        trust_proxy: bool,
        /// Allow outbound tracker/webhook connections to private/local network IPs (for local Gotify/ntfy)
        #[arg(long, env = "PTN_ALLOW_PRIVATE_NETWORKS")]
        allow_private_networks: bool,
    },
    /// Run the background scheduler and WebUI with a system tray icon (alias for `serve --tray`)
    Tray {
        /// Address to bind the WebUI & API server (default 127.0.0.1:8085, or the address
        /// saved in the WebUI). When set, the WebUI cannot change it.
        #[arg(long, env = "PTN_BIND", value_parser = parse_bind_arg)]
        bind: Option<SocketAddr>,
        /// Trust X-Forwarded-For / X-Forwarded-Proto / X-Forwarded-Host headers from a reverse proxy
        #[arg(long, env = "PTN_TRUST_PROXY")]
        trust_proxy: bool,
        /// Allow outbound tracker/webhook connections to private/local network IPs (for local Gotify/ntfy)
        #[arg(long, env = "PTN_ALLOW_PRIVATE_NETWORKS")]
        allow_private_networks: bool,
    },
    /// Run a one-shot check for all enabled trackers (or a specific tracker by name/id)
    Check {
        /// Optional tracker ID or name
        #[arg(long)]
        tracker: Option<String>,
        /// Allow outbound tracker connections to private/local network IPs
        #[arg(long, env = "PTN_ALLOW_PRIVATE_NETWORKS")]
        allow_private_networks: bool,
    },
    /// Send a test notification to all enabled channels (or a specific channel by name/id)
    TestNotify {
        /// Optional channel ID or name
        #[arg(long)]
        channel: Option<String>,
        /// Allow outbound channel connections to private/local network IPs
        #[arg(long, env = "PTN_ALLOW_PRIVATE_NETWORKS")]
        allow_private_networks: bool,
    },
    /// Manage WebUI user accounts
    User {
        #[command(subcommand)]
        action: UserCommands,
    },
}

#[derive(Subcommand)]
enum UserCommands {
    /// Create a new WebUI user account
    Create {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "admin")]
        role: String,
    },
    /// Reset a user's password and revoke all active sessions
    Passwd {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// List configured WebUI user accounts
    List,
    /// Delete a WebUI user account
    Delete {
        #[arg(long)]
        username: String,
    },
}

fn default_db_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(dir) = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .filter(|v| !v.is_empty())
        {
            return PathBuf::from(dir).join("PTNotifier").join("ptnotifier.db");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("PTNotifier")
                .join("ptnotifier.db");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            return PathBuf::from(dir).join("ptnotifier").join("ptnotifier.db");
        }
        if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("ptnotifier")
                .join("ptnotifier.db");
        }
    }

    PathBuf::from("ptnotifier.db")
}

fn build_notify_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(trackers::DEFAULT_USER_AGENT)
        .redirect(security::safe_redirect_policy())
        .build()
        .expect("HTTP client")
}

fn read_password_arg(provided: Option<String>) -> Result<String> {
    if let Some(p) = provided {
        return Ok(p);
    }
    const PROMPT: &str = "Enter password (min 10 chars): ";
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        // Reads from the controlling terminal with echo disabled; the previous
        // terminal mode is restored on both success and failure.
        let pw = rpassword::prompt_password(PROMPT)?;
        return Ok(pw.trim_end_matches(&['\r', '\n'][..]).to_string());
    }
    // Non-interactive input (e.g. piped from a secret manager): nothing to hide.
    eprint!("{PROMPT}");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(&['\r', '\n'][..]).to_string())
}

/// clap value parser sharing the WebUI's validation (rejects port 0, multicast, ...).
fn parse_bind_arg(raw: &str) -> Result<SocketAddr> {
    bind::parse_bind(raw)
}

/// `PTN_BIND` for the default command: unset/blank means "not set", but a present value that
/// does not parse is an error rather than silently falling back to the saved address.
fn env_bind() -> Result<Option<SocketAddr>> {
    match std::env::var("PTN_BIND") {
        Ok(raw) if !raw.trim().is_empty() => bind::parse_bind(&raw)
            .map(Some)
            .map_err(|e| e.context("Invalid PTN_BIND")),
        _ => Ok(None),
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

fn prepare_auth_state(
    db: &Database,
    bind: SocketAddr,
    trust_proxy: bool,
) -> Result<(Arc<std::sync::Mutex<Option<String>>>, bool)> {
    let _ = db.prune_expired_sessions();

    if !bind.ip().is_loopback() && !trust_proxy {
        warn!(
            address = %bind,
            "Binding to a non-loopback address without --trust-proxy. Place PTNotifier behind an HTTPS reverse proxy before exposing it to the internet."
        );
    }

    if db.count_users()? == 0
        && let Ok(admin_pw) = std::env::var("PTN_ADMIN_PASSWORD")
        && !admin_pw.is_empty()
    {
        let admin_user = std::env::var("PTN_ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
        let username = validate_username(&admin_user)?;
        validate_password(&admin_pw)?;
        let pw_hash = hash_password(&admin_pw)?;
        db.create_user(&username, &pw_hash, UserRole::Admin)?;
        info!(username = %username, "Created initial admin account from PTN_ADMIN_PASSWORD");
    }

    if db.count_users()? == 0 {
        let setup_token_required = !bind.ip().is_loopback() || trust_proxy;
        if setup_token_required {
            let raw_token = std::env::var("PTN_SETUP_TOKEN")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| generate_token(24));
            let token_hash = sha256_hex(&raw_token);
            warn!(
                setup_token = %raw_token,
                "No users configured yet. Use this one-time setup token on the WebUI setup screen to create the administrator account."
            );
            return Ok((Arc::new(std::sync::Mutex::new(Some(token_hash))), true));
        }
        info!("No users configured yet. Open the WebUI to create your administrator account.");
    }

    Ok((Arc::new(std::sync::Mutex::new(None)), false))
}

struct WebRuntimeOpts {
    setup_token_hash: Arc<std::sync::Mutex<Option<String>>>,
    setup_token_required: bool,
    trust_proxy: bool,
    allow_private_networks: bool,
    bind: bind::BindInfo,
}

fn build_app_state(
    db: Database,
    scheduler: scheduler::SchedulerHandle,
    event_tx: broadcast::Sender<UiEvent>,
    log_broadcaster: LogBroadcaster,
    notify_client: Client,
    opts: WebRuntimeOpts,
) -> AppState {
    AppState {
        db,
        scheduler,
        event_tx,
        log_broadcaster,
        notify_client,
        auth_limiter: AuthRateLimiter::new(),
        setup_token_hash: opts.setup_token_hash,
        setup_token_required: opts.setup_token_required,
        trust_proxy: opts.trust_proxy,
        allow_private_networks: opts.allow_private_networks,
        bind: opts.bind,
    }
}

fn webui_url_from_bind(bind: SocketAddr) -> String {
    match bind {
        SocketAddr::V4(addr) if addr.ip().is_unspecified() => {
            format!("http://127.0.0.1:{}", addr.port())
        }
        SocketAddr::V6(addr) if addr.ip().is_unspecified() => {
            format!("http://[::1]:{}", addr.port())
        }
        other => format!("http://{other}"),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (event_tx, _) = broadcast::channel::<UiEvent>(512);
    let log_broadcaster = LogBroadcaster::new(event_tx.clone());

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("ptnotifier={},info", cli.log_level)));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(BroadcastLayer::new(log_broadcaster.clone()))
        .init();

    let db_path = cli.db.unwrap_or_else(default_db_path);
    let db = Database::open(&db_path)?;
    let rate_limiter = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(60)));
    let notify_client = build_notify_client();

    let rt = tokio::runtime::Runtime::new()?;

    let default_tray = cfg!(any(
        target_os = "windows",
        target_os = "macos",
        feature = "tray"
    ));
    let cmd = cli.command.unwrap_or(Commands::Serve {
        bind: env_bind()?,
        tray: default_tray,
        trust_proxy: env_flag("PTN_TRUST_PROXY"),
        allow_private_networks: env_flag("PTN_ALLOW_PRIVATE_NETWORKS"),
    });

    match cmd {
        Commands::Serve {
            bind,
            tray: true,
            trust_proxy,
            allow_private_networks,
        }
        | Commands::Tray {
            bind,
            trust_proxy,
            allow_private_networks,
        } => {
            security::set_allow_private_networks(allow_private_networks);
            let (listener, bind_info) = rt.block_on(bind::bind_listener(bind, &db))?;
            let (setup_token_hash, setup_token_required) =
                prepare_auth_state(&db, bind_info.active, trust_proxy)?;

            let (scheduler, app) = rt.block_on(async {
                let scheduler = scheduler::spawn_scheduler(
                    db.clone(),
                    event_tx.clone(),
                    rate_limiter,
                    notify_client.clone(),
                );
                let app = web::router(build_app_state(
                    db.clone(),
                    scheduler.clone(),
                    event_tx,
                    log_broadcaster,
                    notify_client,
                    WebRuntimeOpts {
                        setup_token_hash,
                        setup_token_required,
                        trust_proxy,
                        allow_private_networks,
                        bind: bind_info,
                    },
                ));
                (scheduler, app)
            });

            info!(address = %bind_info.active, db = %db_path.display(), "PTNotifier WebUI listening (tray mode)");
            // tray -> server: Quit selected in the tray menu.
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            // server -> tray: Ctrl+C received, so the tray loop must exit as well.
            let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<()>();

            let server_handle = rt.spawn(async move {
                let shutdown = async {
                    tokio::select! {
                        _ = shutdown_signal() => {
                            let _ = exit_tx.send(());
                        }
                        _ = shutdown_rx => {}
                    }
                };
                if let Err(e) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(shutdown)
                .await
                {
                    error!("WebUI server error: {e}");
                }
            });

            let webui_url = webui_url_from_bind(bind_info.active);
            let tray_res = tray::run_tray(
                webui_url,
                db,
                scheduler,
                rt.handle().clone(),
                shutdown_tx,
                exit_rx,
            );

            rt.block_on(async {
                let _ = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
            });
            tray_res?;
        }
        Commands::Serve {
            bind,
            tray: false,
            trust_proxy,
            allow_private_networks,
        } => {
            security::set_allow_private_networks(allow_private_networks);
            let (listener, bind_info) = rt.block_on(bind::bind_listener(bind, &db))?;
            let (setup_token_hash, setup_token_required) =
                prepare_auth_state(&db, bind_info.active, trust_proxy)?;

            rt.block_on(async {
                let scheduler = scheduler::spawn_scheduler(
                    db.clone(),
                    event_tx.clone(),
                    rate_limiter,
                    notify_client.clone(),
                );

                let app = web::router(build_app_state(
                    db,
                    scheduler,
                    event_tx,
                    log_broadcaster,
                    notify_client,
                    WebRuntimeOpts {
                        setup_token_hash,
                        setup_token_required,
                        trust_proxy,
                        allow_private_networks,
                        bind: bind_info,
                    },
                ));

                info!(address = %bind_info.active, db = %db_path.display(), "PTNotifier WebUI listening");
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(shutdown_signal())
                .await?;
                Ok::<_, anyhow::Error>(())
            })?;
        }
        Commands::Check {
            tracker,
            allow_private_networks,
        } => {
            security::set_allow_private_networks(allow_private_networks);
            rt.block_on(async {
                let list = db.list_trackers()?;
                let targets: Vec<_> = list
                    .into_iter()
                    .filter(|t| {
                        if let Some(ref q) = tracker {
                            t.id == *q || t.name.eq_ignore_ascii_case(q)
                        } else {
                            t.enabled
                        }
                    })
                    .collect();

                if targets.is_empty() {
                    error!("No matching trackers found");
                    std::process::exit(1);
                }

                let mut failed = 0usize;
                for t in &targets {
                    if let Err(e) = scheduler::execute_tracker_check(
                        &db,
                        rate_limiter.clone(),
                        &event_tx,
                        &notify_client,
                        &t.id,
                    )
                    .await
                    {
                        error!(tracker = %t.name, "Check failed: {e}");
                        failed += 1;
                    }
                }
                if failed > 0 {
                    bail!("{failed} of {} tracker check(s) failed", targets.len());
                }
                Ok::<_, anyhow::Error>(())
            })?;
        }
        Commands::TestNotify {
            channel,
            allow_private_networks,
        } => {
            security::set_allow_private_networks(allow_private_networks);
            rt.block_on(async {
                let settings = db.get_settings()?;
                let channels = db.list_channels()?;
                let targets: Vec<_> = channels
                    .into_iter()
                    .filter(|c| {
                        if let Some(ref q) = channel {
                            c.id == *q || c.name.eq_ignore_ascii_case(q)
                        } else {
                            c.enabled
                        }
                    })
                    .collect();

                if targets.is_empty() {
                    error!("No matching channels found");
                    std::process::exit(1);
                }

                let item = NotificationItem {
                    item_type: ItemType::Notification,
                    id: "cli-test".into(),
                    title: Some("PTNotifier CLI Test".into()),
                    subject: Some("Test notification from CLI".into()),
                    sender: Some("PTNotifier".into()),
                    body: Some("Your notification channel works!".into()),
                    date: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    url: "https://github.com/wastaken7/PTNotifier".into(),
                    is_staff: false,
                    favicon: None,
                    mark_read_url: None,
                };

                let mut any_err = false;
                for ch in targets {
                    match notify::send_to_channel(
                        &notify_client,
                        &ch,
                        &settings,
                        "PTNotifier",
                        "https://github.com",
                        &item,
                    )
                    .await
                    {
                        Ok(()) => info!(channel = %ch.name, "Test notification sent"),
                        Err(e) => {
                            error!(channel = %ch.name, "Failed: {e}");
                            any_err = true;
                        }
                    }
                }
                if any_err {
                    std::process::exit(1);
                }
                Ok::<_, anyhow::Error>(())
            })?;
        }
        Commands::User { action } => match action {
            UserCommands::Create {
                username,
                password,
                role,
            } => {
                let clean_user = validate_username(&username)?;
                let parsed_role = match role.trim().to_ascii_lowercase().as_str() {
                    "admin" => UserRole::Admin,
                    "user" => UserRole::User,
                    other => bail!("Invalid role '{other}' (expected 'admin' or 'user')"),
                };
                let pw = read_password_arg(password)?;
                validate_password(&pw)?;
                let hash = hash_password(&pw)?;
                let user = db.create_user(&clean_user, &hash, parsed_role)?;
                println!("Created {} user '{}'", user.role.as_str(), user.username);
            }
            UserCommands::Passwd { username, password } => {
                let Some(user) = db.get_user_by_username(&username)? else {
                    bail!("User '{username}' not found");
                };
                let pw = read_password_arg(password)?;
                validate_password(&pw)?;
                let hash = hash_password(&pw)?;
                db.update_user_password(&user.id, &hash, None)?;
                println!(
                    "Password updated for '{}' and all active sessions revoked.",
                    user.username
                );
            }
            UserCommands::List => {
                let users = db.list_users()?;
                if users.is_empty() {
                    println!("No users configured.");
                } else {
                    for u in users {
                        println!(
                            "{}\t{}\tcreated={}\tlast_login={}",
                            u.username,
                            u.role.as_str(),
                            u.created_at,
                            u.last_login_at.as_deref().unwrap_or("-")
                        );
                    }
                }
            }
            UserCommands::Delete { username } => {
                let Some(user) = db.get_user_by_username(&username)? else {
                    bail!("User '{username}' not found");
                };
                if db.delete_user(&user.id)? {
                    println!("Deleted user '{}'", user.username);
                }
            }
        },
    }

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("Shutdown signal received, stopping...");
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn bind_flag_uses_shared_validation() {
        assert!(Cli::try_parse_from(["ptn", "serve", "--bind", "127.0.0.1:0"]).is_err());
        assert!(Cli::try_parse_from(["ptn", "tray", "--bind", "224.0.0.1:8085"]).is_err());
        assert!(Cli::try_parse_from(["ptn", "serve", "--bind", "127.0.0.1:8090"]).is_ok());
    }
}
