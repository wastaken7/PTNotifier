//! Resolution of the WebUI listen address.
//!
//! Precedence: `--bind` / `PTN_BIND` > address saved from the WebUI > built-in default.
//! A saved address that cannot be bound falls back to the loopback default, so a bad
//! value saved from the WebUI can never leave the app unreachable or unable to start.

use crate::db::Database;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use tokio::net::TcpListener;
use tracing::{error, warn};

pub const DEFAULT_BIND: &str = "127.0.0.1:8085";

pub fn default_bind() -> SocketAddr {
    DEFAULT_BIND.parse().expect("valid default bind")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindSource {
    /// `--bind` or `PTN_BIND`; the WebUI cannot override it.
    Cli,
    /// Address saved from the WebUI.
    Database,
    /// Nothing configured.
    Default,
    /// The saved address failed to bind, so the default was used instead.
    Fallback,
}

#[derive(Clone, Copy, Debug)]
pub struct BindInfo {
    pub active: SocketAddr,
    pub source: BindSource,
}

impl BindInfo {
    pub fn locked(&self) -> bool {
        self.source == BindSource::Cli
    }
}

/// Parses a user-supplied bind address (`IP:port`, IPv6 as `[::]:port`).
pub fn parse_bind(input: &str) -> Result<SocketAddr> {
    let trimmed = input.trim();
    let addr: SocketAddr = trimmed
        .parse()
        .with_context(|| format!("'{trimmed}' is not a valid address (expected IP:port, e.g. 0.0.0.0:8085)"))?;
    if addr.port() == 0 {
        bail!("Port must be between 1 and 65535");
    }
    match addr.ip() {
        IpAddr::V4(ip) if ip.is_multicast() || ip.is_broadcast() => {
            bail!("Multicast/broadcast addresses cannot be bound")
        }
        IpAddr::V6(ip) if ip.is_multicast() => bail!("Multicast addresses cannot be bound"),
        _ => {}
    }
    Ok(addr)
}

/// Checks that `addr` can be bound right now. When the port is the one already in use by
/// this process, only the IP is checked (the port is obviously taken by ourselves).
pub fn probe_bind(addr: SocketAddr, active: SocketAddr) -> Result<()> {
    let target = if addr.port() == active.port() {
        SocketAddr::new(addr.ip(), 0)
    } else {
        addr
    };
    std::net::TcpListener::bind(target)
        .map(drop)
        .with_context(|| format!("Cannot bind {addr}"))
}

/// Picks the address to listen on, without binding it yet.
pub fn resolve(cli_bind: Option<SocketAddr>, db: &Database) -> (SocketAddr, BindSource) {
    if let Some(addr) = cli_bind {
        return (addr, BindSource::Cli);
    }
    match db.get_saved_bind() {
        Ok(Some(saved)) => match parse_bind(&saved) {
            Ok(addr) => (addr, BindSource::Database),
            Err(e) => {
                warn!("Ignoring saved bind address '{saved}': {e:#}");
                (default_bind(), BindSource::Fallback)
            }
        },
        Ok(None) => (default_bind(), BindSource::Default),
        Err(e) => {
            warn!("Could not read saved bind address: {e:#}");
            (default_bind(), BindSource::Default)
        }
    }
}

/// Resolves and binds the WebUI listener, falling back to the default address when a
/// WebUI-saved address cannot be bound.
pub async fn bind_listener(
    cli_bind: Option<SocketAddr>,
    db: &Database,
) -> Result<(TcpListener, BindInfo)> {
    let (addr, source) = resolve(cli_bind, db);
    match TcpListener::bind(addr).await {
        Ok(listener) => Ok((listener, BindInfo { active: addr, source })),
        Err(e) if source == BindSource::Database && addr != default_bind() => {
            error!(
                address = %addr,
                "Could not bind the address saved in the WebUI ({e}); falling back to {DEFAULT_BIND}. Fix it under Settings → Network."
            );
            let fallback = default_bind();
            let listener = TcpListener::bind(fallback)
                .await
                .with_context(|| format!("Cannot bind fallback address {fallback}"))?;
            Ok((
                listener,
                BindInfo {
                    active: fallback,
                    source: BindSource::Fallback,
                },
            ))
        }
        Err(e) => Err(anyhow::Error::new(e).context(format!("Cannot bind {addr}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bind_accepts_v4_v6_and_rejects_bad_input() {
        assert_eq!(parse_bind(" 0.0.0.0:8085 ").unwrap(), "0.0.0.0:8085".parse().unwrap());
        assert!(parse_bind("[::]:9000").is_ok());
        assert!(parse_bind("0.0.0.0").is_err());
        assert!(parse_bind("localhost:8085").is_err());
        assert!(parse_bind("127.0.0.1:0").is_err());
        assert!(parse_bind("224.0.0.1:8085").is_err());
    }

    #[test]
    fn resolve_precedence_and_invalid_saved_value() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(resolve(None, &db).1, BindSource::Default);

        db.set_saved_bind(Some("0.0.0.0:9000")).unwrap();
        assert_eq!(resolve(None, &db), ("0.0.0.0:9000".parse().unwrap(), BindSource::Database));

        let cli: SocketAddr = "127.0.0.1:7000".parse().unwrap();
        assert_eq!(resolve(Some(cli), &db), (cli, BindSource::Cli));

        db.set_saved_bind(Some("garbage")).unwrap();
        assert_eq!(resolve(None, &db), (default_bind(), BindSource::Fallback));

        db.set_saved_bind(None).unwrap();
        assert_eq!(resolve(None, &db).1, BindSource::Default);
    }

    #[test]
    fn probe_rejects_non_local_ip_and_ignores_own_port() {
        // TEST-NET-1: never assigned to a local interface.
        let foreign: SocketAddr = "192.0.2.1:8085".parse().unwrap();
        assert!(probe_bind(foreign, default_bind()).is_err());

        // Port held by "us": only the IP is checked, so this must succeed.
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let active = held.local_addr().unwrap();
        assert!(probe_bind(active, active).is_ok());
        // The same port seen from a process that does not own it is reported as busy.
        assert!(probe_bind(active, default_bind()).is_err());
    }
}
