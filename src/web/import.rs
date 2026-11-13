//! Bulk import of cookie files: one tracker per Netscape `cookies.txt`.
//!
//! The site is identified from the cookies' domain. Fixed-URL presets and the AvistaZ network
//! are detected automatically; generic engines (UNIT3D, NexusPHP) need the type chosen in the
//! request, and their base URL is taken from the cookie domain. When a tracker for the same
//! site already exists, its cookies are replaced instead of creating a duplicate.

use super::{AppState, err_resp, require_admin};
use crate::cookies::{cookie_domains, normalize_host};
use crate::models::{
    AuthRequirement, CreateTrackerRequest, TrackerConfig, TrackerPreset, TrackerPublic,
    UpdateTrackerRequest,
};
use crate::security::validate_outbound_url_with_dns;
use crate::trackers::{TRACKER_PRESETS, find_preset};
use crate::web::auth::AuthContext;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use url::Url;

/// Upper bound on files per request (each one may trigger a DNS lookup).
pub const MAX_IMPORT_FILES: usize = 100;
/// Request body limit for an import (the global API limit is much lower).
pub const MAX_IMPORT_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Upper bound on a single cookie file.
const MAX_FILE_BYTES: usize = 64 * 1024;

/// Sites of the AvistaZ network, all handled by the `avistaz` preset.
const AVISTAZ_NETWORK: &[&str] = &["avistaz.to", "cinemaz.to", "privatehd.to", "exoticaz.to", "animez.to"];

#[derive(Deserialize)]
pub struct ImportRequest {
    /// Tracker type for every file; `None`/empty = detect from the cookie domain.
    #[serde(default)]
    kind: Option<String>,
    /// Replace the cookies of an existing tracker for the same site (default) instead of
    /// skipping the file.
    #[serde(default = "default_true")]
    update_existing: bool,
    files: Vec<ImportFile>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct ImportFile {
    name: String,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Created,
    Updated,
    Skipped,
    Error,
}

#[derive(Serialize)]
pub struct ImportResult {
    file: String,
    status: ImportStatus,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracker: Option<TrackerPublic>,
}

/// What to do with one file.
#[derive(Debug, PartialEq)]
enum Plan {
    Create {
        kind: &'static str,
        name: String,
        base_url: String,
    },
    Update {
        id: String,
        name: String,
    },
}

fn host_of(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(normalize_host)
}

/// Same site, allowing a subdomain on either side (`btschool.club` vs `pt.btschool.club`).
fn same_site(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
}

fn accepts_cookies(preset: &TrackerPreset) -> bool {
    matches!(
        preset.auth,
        AuthRequirement::Cookies | AuthRequirement::CookiesAndApiToken
    )
}

/// Finds the preset for a cookie domain, if the site has one of its own.
fn detect_preset(domain: &str) -> Option<&'static TrackerPreset> {
    if AVISTAZ_NETWORK.iter().any(|d| same_site(domain, d)) {
        return find_preset("avistaz");
    }
    TRACKER_PRESETS.iter().find(|p| {
        !p.custom_base_url
            && accepts_cookies(p)
            && host_of(p.default_base_url).is_some_and(|h| same_site(domain, &h))
    })
}

/// "aither.cc" -> "Aither", "pt.btschool.club" -> "Btschool".
fn name_from_domain(domain: &str) -> String {
    let labels: Vec<&str> = domain.split('.').collect();
    let label = if labels.len() >= 2 { labels[labels.len() - 2] } else { domain };
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => domain.to_string(),
    }
}

fn plan_file(kind: Option<&str>, content: &str, existing: &[TrackerConfig]) -> Result<Plan, String> {
    let domains = cookie_domains(content);
    let Some(domain) = domains.first() else {
        return Err(
            "No cookies in Netscape cookies.txt format found (raw 'Cookie:' headers carry no domain; add those with \"Add tracker\")"
                .into(),
        );
    };

    let preset = match kind {
        Some(k) => {
            let preset = find_preset(k).ok_or_else(|| format!("Unknown tracker type '{k}'"))?;
            if !accepts_cookies(preset) {
                return Err(format!("{} does not use cookies", preset.label));
            }
            // A fixed-URL preset must match the cookies, or they belong to another site.
            if !preset.custom_base_url
                && let Some(host) = host_of(preset.default_base_url)
                && !same_site(domain, &host)
            {
                return Err(format!("Cookies are for {domain}, not {host}"));
            }
            // Generic types accept any domain; don't turn a site that has its own preset
            // (e.g. BJ-Share in a folder imported as UNIT3D) into a mis-typed tracker.
            if let Some(own) = detect_preset(domain)
                && own.kind != preset.kind
            {
                return Err(format!(
                    "{domain} is {}, not {}; import it with Auto-detect",
                    own.label, preset.label
                ));
            }
            preset
        }
        None => detect_preset(domain).ok_or_else(|| {
            format!("Could not detect the tracker type for {domain}; choose a type (e.g. UNIT3D) and import again")
        })?,
    };

    let base_url = if preset.custom_base_url {
        format!("https://{domain}/")
    } else {
        preset.default_base_url.to_string()
    };

    let base_host = host_of(&base_url).unwrap_or_else(|| domain.clone());
    if let Some(t) = existing.iter().find(|t| {
        t.kind.eq_ignore_ascii_case(preset.kind)
            && host_of(&t.base_url).is_some_and(|h| same_site(&h, &base_host))
    }) {
        return Ok(Plan::Update {
            id: t.id.clone(),
            name: t.name.clone(),
        });
    }

    let name = if preset.custom_base_url {
        name_from_domain(domain)
    } else {
        preset.label.split(" (").next().unwrap_or(preset.label).to_string()
    };
    Ok(Plan::Create {
        kind: preset.kind,
        name,
        base_url,
    })
}

fn cookies_only_update(cookies: String) -> UpdateTrackerRequest {
    UpdateTrackerRequest {
        name: None,
        kind: None,
        base_url: None,
        enabled: None,
        check_interval_secs: None,
        mark_as_read: None,
        cookies_raw: Some(cookies),
        api_token: None,
        clear_cookies: false,
        clear_api_token: false,
        ignore_patterns: None,
    }
}

pub async fn import_cookies(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<ImportRequest>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    if payload.files.is_empty() {
        return err_resp(StatusCode::BAD_REQUEST, "No files to import");
    }
    if payload.files.len() > MAX_IMPORT_FILES {
        return err_resp(
            StatusCode::BAD_REQUEST,
            format!("Too many files (max {MAX_IMPORT_FILES} per import)"),
        );
    }
    let kind = payload
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty() && !k.eq_ignore_ascii_case("auto"));

    let mut results = Vec::with_capacity(payload.files.len());
    let mut changed = false;

    for file in payload.files {
        let file_name: String = file.name.chars().take(200).collect();
        let result = |status, message: String, tracker| ImportResult {
            file: file_name.clone(),
            status,
            message,
            tracker,
        };

        if file.content.len() > MAX_FILE_BYTES {
            results.push(result(ImportStatus::Error, "File is too large for a cookie file".into(), None));
            continue;
        }
        // Re-read every time so two files for the same site in one batch update one tracker.
        let existing = match state.db.list_trackers() {
            Ok(t) => t,
            Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };

        let outcome = match plan_file(kind, &file.content, &existing) {
            Err(msg) => result(ImportStatus::Error, msg, None),
            Ok(Plan::Update { name, .. }) if !payload.update_existing => result(
                ImportStatus::Skipped,
                format!("Tracker '{name}' already exists"),
                None,
            ),
            Ok(Plan::Update { id, name }) => {
                match state.db.update_tracker(&id, cookies_only_update(file.content)) {
                    Ok(Some(t)) => {
                        changed = true;
                        result(
                            ImportStatus::Updated,
                            format!("Cookies of '{name}' replaced"),
                            Some(t.to_public()),
                        )
                    }
                    Ok(None) => result(ImportStatus::Error, "Tracker disappeared".into(), None),
                    Err(e) => result(ImportStatus::Error, e.to_string(), None),
                }
            }
            Ok(Plan::Create { kind, name, base_url }) => {
                if let Err(e) =
                    validate_outbound_url_with_dns(&base_url, state.allow_private_networks, false).await
                {
                    result(ImportStatus::Error, format!("Invalid base URL {base_url}: {e}"), None)
                } else {
                    let req = CreateTrackerRequest {
                        name: name.clone(),
                        kind: kind.to_string(),
                        base_url: Some(base_url.clone()),
                        enabled: true,
                        check_interval_secs: None,
                        mark_as_read: None,
                        cookies_raw: file.content,
                        api_token: String::new(),
                        ignore_patterns: Vec::new(),
                    };
                    match state.db.create_tracker(req, base_url) {
                        Ok(t) => {
                            changed = true;
                            result(
                                ImportStatus::Created,
                                format!("Created '{name}'"),
                                Some(t.to_public()),
                            )
                        }
                        Err(e) => result(ImportStatus::Error, e.to_string(), None),
                    }
                }
            }
        };
        results.push(outcome);
    }

    if changed {
        state.scheduler.reload().await;
    }
    Json(serde_json::json!({ "results": results })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn netscape(domain: &str) -> String {
        format!("# Netscape HTTP Cookie File\n.{domain}\tTRUE\t/\tTRUE\t0\tsession\tabc\n")
    }

    #[test]
    fn detects_fixed_presets_and_avistaz_network() {
        assert_eq!(
            plan_file(None, &netscape("bj-share.info"), &[]).unwrap(),
            Plan::Create {
                kind: "bjshare",
                name: "BJ-Share".into(),
                base_url: "https://bj-share.info/".into()
            }
        );
        // Cookie on the parent domain of a preset hosted on a subdomain.
        assert!(matches!(
            plan_file(None, &netscape("btschool.club"), &[]).unwrap(),
            Plan::Create { kind: "btschool", .. }
        ));
        assert_eq!(
            plan_file(None, &netscape("privatehd.to"), &[]).unwrap(),
            Plan::Create {
                kind: "avistaz",
                name: "Privatehd".into(),
                base_url: "https://privatehd.to/".into()
            }
        );
    }

    #[test]
    fn generic_engines_need_a_type_and_take_the_url_from_cookies() {
        assert!(plan_file(None, &netscape("aither.cc"), &[]).is_err());
        assert_eq!(
            plan_file(Some("unit3d"), &netscape("aither.cc"), &[]).unwrap(),
            Plan::Create {
                kind: "unit3d",
                name: "Aither".into(),
                base_url: "https://aither.cc/".into()
            }
        );
        // Fixed preset chosen for another site's cookies.
        assert!(plan_file(Some("bjshare"), &netscape("aither.cc"), &[]).is_err());
        // Generic type chosen for a site that has its own preset.
        assert!(plan_file(Some("unit3d"), &netscape("bj-share.info"), &[]).is_err());
        assert!(plan_file(Some("unit3d"), &netscape("privatehd.to"), &[]).is_err());
        assert!(plan_file(Some("avistaz"), &netscape("cinemaz.to"), &[]).is_ok());
        // API-token-only preset.
        assert!(plan_file(Some("anthelion"), &netscape("anthelion.me"), &[]).is_err());
        // Raw header: no domain to work with.
        assert!(plan_file(Some("unit3d"), "Cookie: a=1", &[]).is_err());
    }

    #[test]
    fn existing_tracker_for_the_same_site_is_updated() {
        let db = Database::open_in_memory().unwrap();
        let t = db
            .create_tracker(
                CreateTrackerRequest {
                    name: "My Aither".into(),
                    kind: "unit3d".into(),
                    base_url: None,
                    enabled: true,
                    check_interval_secs: None,
                    mark_as_read: None,
                    cookies_raw: String::new(),
                    api_token: String::new(),
                    ignore_patterns: vec![],
                },
                "https://www.aither.cc".into(),
            )
            .unwrap();
        let existing = db.list_trackers().unwrap();
        assert_eq!(
            plan_file(Some("unit3d"), &netscape("aither.cc"), &existing).unwrap(),
            Plan::Update {
                id: t.id,
                name: "My Aither".into()
            }
        );
        // Same domain, different engine: not the same tracker.
        assert!(matches!(
            plan_file(Some("nexusphp"), &netscape("aither.cc"), &existing).unwrap(),
            Plan::Create { .. }
        ));
    }
}
