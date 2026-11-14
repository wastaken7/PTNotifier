//! Admin endpoints for server-level options that apply on restart (WebUI bind address).

use crate::bind::{self, BindSource, DEFAULT_BIND};
use crate::security::{RateBucket, verify_password};
use crate::web::auth::{AuthContext, extract_client_ip, rate_limited_resp};
use crate::web::{AppState, err_resp, require_admin};
use axum::extract::{ConnectInfo, Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tracing::{info, warn};

#[derive(Serialize)]
struct BindStatus {
    /// Address the server is listening on right now.
    active: String,
    source: BindSource,
    /// Address saved from the WebUI, if any.
    saved: Option<String>,
    default: &'static str,
    /// Set by `--bind` / `PTN_BIND`; the WebUI cannot change it.
    locked: bool,
    /// Address that will be used on the next start.
    next: String,
    restart_required: bool,
    trust_proxy: bool,
}

fn bind_status(state: &AppState) -> anyhow::Result<BindStatus> {
    let saved = state.db.get_saved_bind()?;
    let info = state.bind;
    let next = if info.locked() {
        info.active
    } else {
        saved
            .as_deref()
            .and_then(|s| bind::parse_bind(s).ok())
            .unwrap_or_else(bind::default_bind)
    };
    Ok(BindStatus {
        active: info.active.to_string(),
        source: info.source,
        saved,
        default: DEFAULT_BIND,
        locked: info.locked(),
        next: next.to_string(),
        restart_required: next != info.active,
        trust_proxy: state.trust_proxy,
    })
}

pub async fn get_bind(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    match bind_status(&state) {
        Ok(s) => Json(s).into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct UpdateBindRequest {
    /// `None` or empty resets to the default (loopback) address.
    bind: Option<String>,
    /// Current password of the signed-in admin, required because this widens network exposure.
    password: String,
}

pub async fn update_bind(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(payload): Json<UpdateBindRequest>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    // Password re-check endpoint: same brute-force budget as login.
    let client_ip = extract_client_ip(&headers, peer.map(|c| c.0.0), state.trust_proxy);
    if let Err(retry) = state.auth_limiter.check_ip_rate(client_ip, RateBucket::Auth) {
        return rate_limited_resp(retry);
    }
    if !verify_password(&payload.password, &auth.user.password_hash) {
        // Not 401: the session is still valid, and the WebUI treats 401 as "signed out".
        return err_resp(StatusCode::BAD_REQUEST, "Password is incorrect");
    }
    if state.bind.locked() {
        return err_resp(
            StatusCode::CONFLICT,
            "The listen address is set by --bind / PTN_BIND and cannot be changed from the WebUI",
        );
    }

    let requested = payload
        .bind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let to_save = match requested {
        None => {
            // The default address is bound without fallback at startup, so make sure it is
            // usable before dropping the saved address.
            if let Err(e) = bind::probe_bind(bind::default_bind(), state.bind.active) {
                return err_resp(StatusCode::BAD_REQUEST, format!("{e:#}"));
            }
            None
        }
        Some(raw) => {
            let addr = match bind::parse_bind(raw) {
                Ok(a) => a,
                Err(e) => return err_resp(StatusCode::BAD_REQUEST, format!("{e:#}")),
            };
            if let Err(e) = bind::probe_bind(addr, state.bind.active) {
                return err_resp(StatusCode::BAD_REQUEST, format!("{e:#}"));
            }
            // Saving the default explicitly is the same as clearing the override.
            (addr != bind::default_bind()).then(|| addr.to_string())
        }
    };

    if let Err(e) = state.db.set_saved_bind(to_save.as_deref()) {
        return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    let effective = to_save.as_deref().unwrap_or(DEFAULT_BIND);
    info!(
        username = %auth.user.username,
        address = %effective,
        "WebUI listen address changed; takes effect on restart"
    );
    if let Ok(addr) = bind::parse_bind(effective)
        && !addr.ip().is_loopback()
        && !state.trust_proxy
    {
        warn!(
            address = %addr,
            "The WebUI will be reachable from other machines after restart. Use HTTPS via a reverse proxy before exposing it to the internet."
        );
    }

    match bind_status(&state) {
        Ok(s) => Json(s).into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
