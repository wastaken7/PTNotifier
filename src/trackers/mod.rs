pub mod avistaz;
pub mod gazelle;
pub mod misc;
pub mod nexusphp;
pub mod unit3d;
pub mod xbtit;

use crate::cookies::build_cookie_jar;
use crate::models::{
    AuthRequirement, GlobalSettings, NotificationItem, TrackerConfig, TrackerPreset, TrackerState,
};
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use scraper::{ElementRef, Selector};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use url::Url;

pub const DEFAULT_USER_AGENT: &str = "PTNotifier/0.1 (https://github.com/wastaken7/PTNotifier)";
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug)]
pub enum TrackerError {
    CookieExpired(String),
    Config(String),
    Network(String),
    Parse(String),
}

impl fmt::Display for TrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CookieExpired(m) => write!(f, "Cookie expired / invalid session: {m}"),
            Self::Config(m) => write!(f, "Configuration error: {m}"),
            Self::Network(m) => write!(f, "Network error: {m}"),
            Self::Parse(m) => write!(f, "Parse error: {m}"),
        }
    }
}

impl std::error::Error for TrackerError {}

/// A transport error without its request URL: tracker URLs can carry API keys or passkeys
/// (e.g. Anthelion's `api_key`), and these messages end up in logs and error alerts.
pub(crate) fn net_error(context: &str, e: reqwest::Error) -> TrackerError {
    TrackerError::Network(format!("{context}: {}", e.without_url()))
}

pub static TRACKER_PRESETS: &[TrackerPreset] = &[
    TrackerPreset {
        kind: "unit3d",
        label: "UNIT3D (Generic)",
        default_base_url: "https://example.org",
        custom_base_url: true,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "Works for any standard UNIT3D tracker. Set your tracker's base URL.",
    },
    TrackerPreset {
        kind: "hawkeuno",
        label: "Hawke-Uno (Deep Space)",
        default_base_url: "https://hawke.uno",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "Custom UNIT3D Deep Space layout.",
    },
    TrackerPreset {
        kind: "avistaz",
        label: "AvistaZ Network (AvistaZ / CinemaZ / PrivateHD / ExoticaZ / AnimeZ)",
        default_base_url: "https://avistaz.to/",
        custom_base_url: true,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "Set base URL to your AvistaZ network site (e.g. https://privatehd.to/).",
    },
    TrackerPreset {
        kind: "nexusphp",
        label: "NexusPHP (Generic)",
        default_base_url: "https://example.org/",
        custom_base_url: true,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "Works for any standard NexusPHP tracker.",
    },
    TrackerPreset {
        kind: "btschool",
        label: "BTSCHOOL (NexusPHP)",
        default_base_url: "https://pt.btschool.club/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "baozi",
        label: "Baozi (NexusPHP)",
        default_base_url: "https://p.t-baozi.cc/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "lajidui",
        label: "Lajidui (NexusPHP)",
        default_base_url: "https://pt.lajidui.top/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "longpt",
        label: "LongPT (NexusPHP)",
        default_base_url: "https://longpt.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "march",
        label: "March (NexusPHP)",
        default_base_url: "https://duckboobee.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "ptcafe",
        label: "PTCafe (NexusPHP)",
        default_base_url: "https://ptcafe.club/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "ptfans",
        label: "PTFans (NexusPHP)",
        default_base_url: "https://ptfans.cc/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "ptgtk",
        label: "PTGTK (NexusPHP)",
        default_base_url: "https://pt.gtkpw.xyz/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "ptskit",
        label: "PTSKit (NexusPHP)",
        default_base_url: "https://www.ptskit.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "railgunpt",
        label: "RailgunPT (NexusPHP)",
        default_base_url: "https://bilibili.download/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "bjshare",
        label: "BJ-Share",
        default_base_url: "https://bj-share.info/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 3600,
        notes: "Minimum interval 3600s enforced by site admin.",
    },
    TrackerPreset {
        kind: "brasiltracker",
        label: "BrasilTracker",
        default_base_url: "https://brasiltracker.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "greatposterwall",
        label: "GreatPosterWall",
        default_base_url: "https://greatposterwall.com/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "orpheus",
        label: "Orpheus",
        default_base_url: "https://orpheus.network/",
        custom_base_url: false,
        auth: AuthRequirement::CookiesAndApiToken,
        min_interval_secs: 1800,
        notes: "Requires API token (and optionally cookies).",
    },
    TrackerPreset {
        kind: "anthelion",
        label: "Anthelion",
        default_base_url: "https://anthelion.me/",
        custom_base_url: false,
        auth: AuthRequirement::ApiToken,
        min_interval_secs: 1800,
        notes: "API-only tracker. Only User API key is required.",
    },
    TrackerPreset {
        kind: "hdspace",
        label: "HD-Space",
        default_base_url: "https://hd-space.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "hdtorrents",
        label: "HD-Torrents",
        default_base_url: "https://hd-torrents.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "sportscult",
        label: "SportsCult",
        default_base_url: "https://sportscult.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "amigosshareclub",
        label: "Amigos Share Club (ASC)",
        default_base_url: "https://cliente.amigos-share.club/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "btarg",
        label: "BTArg",
        default_base_url: "https://www.btarg.com.ar/tracker/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "digitalcore",
        label: "DigitalCore",
        default_base_url: "https://digitalcore.club/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "fappaizuri",
        label: "Fappaizuri",
        default_base_url: "https://fappaizuri.me/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "hdcity",
        label: "HDCiTY",
        default_base_url: "https://hdcity.city/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "iptorrents",
        label: "IPTorrents",
        default_base_url: "https://iptorrents.com/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "immortalseed",
        label: "ImmortalSeed",
        default_base_url: "https://immortalseed.me/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "scenetime",
        label: "SceneTime",
        default_base_url: "https://www.scenetime.com/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "torrentday",
        label: "TorrentDay",
        default_base_url: "https://www.torrentday.com/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "torrentleech",
        label: "TorrentLeech",
        default_base_url: "https://www.torrentleech.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
    TrackerPreset {
        kind: "yggreborn",
        label: "YGG Reborn",
        default_base_url: "https://www.yggreborn.org/",
        custom_base_url: false,
        auth: AuthRequirement::Cookies,
        min_interval_secs: 1800,
        notes: "",
    },
];

pub fn find_preset(kind: &str) -> Option<&'static TrackerPreset> {
    TRACKER_PRESETS
        .iter()
        .find(|p| p.kind.eq_ignore_ascii_case(kind))
}

pub struct TrackerContext {
    pub client: Client,
    pub config: TrackerConfig,
    pub state: TrackerState,
    pub settings: GlobalSettings,
    pub rate_limiter: Arc<Mutex<Instant>>,
}

impl TrackerContext {
    pub fn new(
        config: TrackerConfig,
        settings: GlobalSettings,
        rate_limiter: Arc<Mutex<Instant>>,
    ) -> Result<Self, TrackerError> {
        let mut headers = HeaderMap::new();
        let ua = if config.kind.eq_ignore_ascii_case("hdtorrents") {
            BROWSER_USER_AGENT
        } else {
            DEFAULT_USER_AGENT
        };
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(ua),
        );

        let jar = build_cookie_jar(&config.cookies_raw, &config.base_url);
        let timeout = Duration::from_secs_f64(settings.http_timeout_secs.max(5.0));

        let client = Client::builder()
            .default_headers(headers)
            .cookie_provider(jar)
            .timeout(timeout)
            .redirect(crate::security::safe_redirect_policy())
            .dns_resolver(Arc::new(crate::security::SsrfSafeResolver))
            .build()
            .map_err(|e| TrackerError::Config(format!("failed to build HTTP client: {e}")))?;

        let state = config.state.clone();
        Ok(Self {
            client,
            config,
            state,
            settings,
            rate_limiter,
        })
    }

    pub async fn respect_rate_limit(&self) {
        let delay = Duration::from_secs_f64(self.settings.request_delay_secs.max(0.0));
        if delay.is_zero() {
            return;
        }
        let mut last = self.rate_limiter.lock().await;
        let elapsed = last.elapsed();
        if elapsed < delay {
            tokio::time::sleep(delay - elapsed).await;
        }
        *last = Instant::now();
    }

    pub async fn fetch_page(
        &self,
        url: &str,
        context: &str,
        success_text: Option<&str>,
    ) -> Result<String, TrackerError> {
        self.respect_rate_limit().await;
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| net_error(context, e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| net_error(&format!("{context} body read"), e))?;

        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(TrackerError::CookieExpired(format!(
                    "{context}: HTTP {status}"
                )));
            }
            return Err(TrackerError::Network(format!("{context}: HTTP {status}")));
        }

        if let Some(kw) = success_text
            && !kw.is_empty()
            && !body.contains(kw)
        {
            return Err(TrackerError::CookieExpired(format!(
                "{context}: expected marker '{kw}' not found (expired cookies or HTML changed)"
            )));
        }

        Ok(body)
    }

    pub fn join_url(&self, rel: &str) -> String {
        if rel.starts_with("http://") || rel.starts_with("https://") {
            return rel.to_string();
        }
        if let Ok(base) = Url::parse(&self.config.base_url)
            && let Ok(joined) = base.join(rel)
        {
            return joined.to_string();
        }
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            rel.trim_start_matches('/')
        )
    }

    pub fn is_processed(&self, id: &str) -> bool {
        self.state.processed_ids.iter().any(|x| x == id)
    }

    pub fn mark_processed(&mut self, id: &str) {
        if !self.is_processed(id) {
            self.state.processed_ids.push(id.to_string());
        }
    }

    pub fn should_mark_as_read(&self) -> bool {
        self.config.mark_as_read.unwrap_or(self.settings.mark_as_read)
    }
}

pub fn text_clean(el: ElementRef<'_>) -> String {
    el.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("valid static CSS selector")
}

pub async fn fetch_items(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let kind = ctx.config.kind.to_lowercase();
    match kind.as_str() {
        "unit3d" => unit3d::fetch_items(ctx, false).await,
        "hawkeuno" => unit3d::fetch_items(ctx, true).await,
        "nexusphp" | "btschool" | "baozi" | "lajidui" | "longpt" | "march" | "ptcafe"
        | "ptfans" | "ptgtk" | "ptskit" | "railgunpt" => nexusphp::fetch_items(ctx).await,
        "avistaz" => avistaz::fetch_items(ctx).await,
        "bjshare" => gazelle::fetch_bjshare_or_brasil(ctx, true).await,
        "brasiltracker" => gazelle::fetch_bjshare_or_brasil(ctx, false).await,
        "greatposterwall" => gazelle::fetch_gpw(ctx).await,
        "orpheus" => gazelle::fetch_orpheus(ctx).await,
        "anthelion" => gazelle::fetch_anthelion(ctx).await,
        "hdspace" => xbtit::fetch_items(ctx, xbtit::XbtitVariant::HdSpace).await,
        "hdtorrents" => xbtit::fetch_items(ctx, xbtit::XbtitVariant::HdTorrents).await,
        "sportscult" => xbtit::fetch_items(ctx, xbtit::XbtitVariant::SportsCult).await,
        "amigosshareclub" => misc::fetch_asc(ctx).await,
        "btarg" => misc::fetch_btarg(ctx).await,
        "digitalcore" => misc::fetch_digitalcore(ctx).await,
        "fappaizuri" => misc::fetch_fappaizuri(ctx).await,
        "hdcity" => misc::fetch_hdcity(ctx).await,
        "iptorrents" => misc::fetch_iptorrents(ctx).await,
        "immortalseed" => misc::fetch_immortalseed(ctx).await,
        "scenetime" => misc::fetch_scenetime(ctx).await,
        "torrentday" => misc::fetch_torrentday(ctx).await,
        "torrentleech" => misc::fetch_torrentleech(ctx).await,
        "yggreborn" => misc::fetch_yggreborn(ctx).await,
        other => Err(TrackerError::Config(format!("Unknown tracker kind: {other}"))),
    }
}

pub async fn mark_item_read(ctx: &TrackerContext, item: &NotificationItem) {
    if !ctx.should_mark_as_read() {
        return;
    }
    let kind = ctx.config.kind.to_lowercase();
    if (kind == "unit3d" || kind == "hawkeuno")
        && let Some(url) = &item.mark_read_url
    {
        unit3d::mark_notification_read(ctx, url).await;
    }
}

#[cfg(test)]
mod net_error_tests {
    use super::net_error;

    #[tokio::test]
    async fn transport_errors_do_not_leak_the_url() {
        // Port 9 on loopback refuses the connection, so this fails without leaving the machine.
        let err = reqwest::Client::new()
            .get("http://127.0.0.1:9/api.php?api_key=SECRET_KEY")
            .send()
            .await
            .unwrap_err();
        assert!(
            format!("{err}").contains("SECRET_KEY"),
            "precondition: reqwest includes the URL"
        );
        let msg = net_error("Anthelion", err).to_string();
        assert!(!msg.contains("SECRET_KEY"), "{msg}");
    }
}
