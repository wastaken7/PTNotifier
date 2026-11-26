//! Checks GitHub for a newer PTNotifier release, so the WebUI can say when one is out.
//!
//! The only request goes to the GitHub API (never to a tracker). It runs on demand, when
//! the WebUI asks, and the result is cached so page loads don't hit GitHub each time.
//! Nothing is downloaded or installed: the WebUI only shows a link to the release page.

use super::AppState;
use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::debug;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/wastaken7/PTNotifier/releases/latest";
/// Fixed link for the WebUI: the API's own `html_url` is never put in the page.
const RELEASES_PAGE: &str = "https://github.com/wastaken7/PTNotifier/releases/latest";

const CACHE_OK: Duration = Duration::from_secs(6 * 60 * 60);
const CACHE_ERR: Duration = Duration::from_secs(60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Last answer from GitHub: `Some(tag)` on success, `None` when the check failed.
type CachedTag = (Instant, Option<String>);

#[derive(Clone, Default)]
pub struct UpdateCache(Arc<Mutex<Option<CachedTag>>>);

#[derive(Serialize)]
struct VersionInfo {
    current: &'static str,
    check_enabled: bool,
    latest: Option<String>,
    update_available: bool,
    url: Option<&'static str>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn get_version(State(state): State<AppState>) -> Response {
    let check_enabled = state
        .db
        .get_settings()
        .map(|s| s.check_for_updates)
        .unwrap_or(false);
    let latest = if check_enabled {
        latest_tag(&state.update_cache, &state.notify_client).await
    } else {
        None
    };
    let update_available = latest
        .as_deref()
        .is_some_and(|tag| compare_versions(tag, CURRENT_VERSION) == Some(Ordering::Greater));
    Json(VersionInfo {
        current: CURRENT_VERSION,
        check_enabled,
        latest,
        update_available,
        url: update_available.then_some(RELEASES_PAGE),
    })
    .into_response()
}

async fn latest_tag(cache: &UpdateCache, client: &Client) -> Option<String> {
    // Held across the request so concurrent page loads share one GitHub call.
    let mut guard = cache.0.lock().await;
    if let Some((at, tag)) = guard.as_ref() {
        let ttl = if tag.is_some() { CACHE_OK } else { CACHE_ERR };
        if at.elapsed() < ttl {
            return tag.clone();
        }
    }
    let tag = match fetch_latest_tag(client).await {
        Ok(tag) => Some(tag),
        Err(e) => {
            debug!("Update check failed: {e}");
            None
        }
    };
    *guard = Some((Instant::now(), tag.clone()));
    tag
}

async fn fetch_latest_tag(client: &Client) -> anyhow::Result<String> {
    let release: Release = client
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tag = release.tag_name.trim();
    if tag.is_empty() || tag.len() > 64 || parse_version(tag).is_none() {
        anyhow::bail!("unexpected release tag");
    }
    Ok(tag.to_string())
}

/// `v1.2.3` / `1.2` → `[1, 2, 3]`. Pre-release or build suffixes (`-beta`, `+x`) make it
/// `None`: those are never offered as updates.
fn parse_version(v: &str) -> Option<Vec<u64>> {
    let v = v.strip_prefix(['v', 'V']).unwrap_or(v);
    let parts: Option<Vec<u64>> = v.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty() && p.len() <= 4)
}

fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let (mut a, mut b) = (parse_version(a)?, parse_version(b)?);
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    Some(a.cmp(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_release_tags() {
        assert_eq!(compare_versions("v0.2.0", "0.1.0"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v0.1.10", "0.1.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v0.1", "0.1.0"), Some(Ordering::Equal));
        assert_eq!(compare_versions("0.1.0", "0.2.0"), Some(Ordering::Less));
        assert_eq!(compare_versions("v1.0.0-beta", "0.1.0"), None);
        assert_eq!(compare_versions("nightly", "0.1.0"), None);
        assert_eq!(compare_versions("", "0.1.0"), None);
    }

    #[test]
    fn current_version_parses() {
        assert!(parse_version(CURRENT_VERSION).is_some());
    }
}
