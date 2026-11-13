pub mod auth;
mod rss;

use crate::db::Database;
use crate::logging::LogBroadcaster;
use crate::models::{
    CreateChannelRequest, CreateTrackerRequest, GlobalSettings, ItemType, NotificationItem,
    TrackerPublic, UiEvent, UpdateChannelRequest, UpdateTrackerRequest, UserRole,
};
use crate::notify;
use crate::scheduler::SchedulerHandle;
use crate::security::{
    AuthRateLimiter, RateBucket, is_safe_http_url, sha256_hex, validate_channel_config_urls,
    validate_outbound_url_with_dns,
};
use crate::trackers::{TRACKER_PRESETS, find_preset};
use auth::{AuthContext, extract_bearer_token, extract_client_ip, extract_session_token};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{delete, get, post, put};
use axum::{Router, response::Response as AxumResponse};
use chrono::Utc;
use futures::stream::Stream;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

const INDEX_HTML: &str = include_str!("assets/index.html");
const STYLE_CSS: &str = include_str!("assets/style.css");
const APP_JS: &str = include_str!("assets/app.js");
const ICON_SVG: &str = include_str!("assets/icon.svg");
const ICON_PNG: &[u8] = include_bytes!("assets/icon.png");

const MAX_BODY_BYTES: usize = 512 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub scheduler: SchedulerHandle,
    pub event_tx: broadcast::Sender<UiEvent>,
    pub log_broadcaster: LogBroadcaster,
    pub notify_client: Client,
    pub auth_limiter: AuthRateLimiter,
    pub setup_token_hash: Arc<Mutex<Option<String>>>,
    pub setup_token_required: bool,
    pub trust_proxy: bool,
    pub allow_private_networks: bool,
}

pub fn router(state: AppState) -> Router {
    let protected_api = Router::new()
        .route("/api/overview", get(overview))
        .route("/api/settings", get(get_settings).put(update_settings))
        .route("/api/trackers/presets", get(get_presets))
        .route("/api/trackers", get(list_trackers).post(create_tracker))
        .route(
            "/api/trackers/{id}",
            put(update_tracker).delete(delete_tracker),
        )
        .route("/api/trackers/{id}/run", post(run_tracker_now))
        .route("/api/channels", get(list_channels).post(create_channel))
        .route(
            "/api/channels/{id}",
            put(update_channel).delete(delete_channel),
        )
        .route("/api/channels/{id}/test", post(test_channel))
        .route("/api/notifications", get(list_notifications))
        .route("/api/logs", get(list_logs))
        .route("/api/events", get(sse_events))
        .route("/api/auth/password", post(auth::change_password))
        .route("/api/auth/sessions", get(auth::list_sessions))
        .route("/api/auth/sessions/others", post(auth::revoke_other_sessions))
        .route("/api/auth/sessions/{prefix}", delete(auth::revoke_session))
        .route(
            "/api/auth/rss-tokens",
            get(auth::list_rss_tokens).post(auth::create_rss_token),
        )
        .route("/api/auth/rss-tokens/{id}", delete(auth::delete_rss_token))
        .route("/api/users", get(auth::list_users).post(auth::create_user))
        .route("/api/users/{id}", delete(auth::delete_user))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth_middleware,
        ));

    Router::new()
        .route("/", get(serve_index))
        .route("/assets/style.css", get(serve_css))
        .route("/assets/app.js", get(serve_js))
        .route("/assets/icon.svg", get(serve_icon_svg))
        .route("/assets/icon.png", get(serve_icon_png))
        .route("/api/health", get(health))
        .route("/api/auth/status", get(auth::auth_status))
        .route("/api/auth/setup", post(auth::setup_admin))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/rss", get(serve_rss))
        .route("/api/rss", get(serve_rss))
        .merge(protected_api)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::security_headers_middleware,
        ))
        .with_state(state)
}

pub fn err_resp(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

fn require_admin(auth: &AuthContext) -> Option<AxumResponse> {
    if auth.user.role != UserRole::Admin {
        return Some(err_resp(
            StatusCode::FORBIDDEN,
            "Administrator privileges required for this action",
        ));
    }
    None
}

async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn serve_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLE_CSS)
}

async fn serve_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        APP_JS,
    )
}

async fn serve_icon_svg() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], ICON_SVG)
}

async fn serve_icon_png() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/png")], ICON_PNG)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn overview(State(state): State<AppState>) -> Response {
    let settings = match state.db.get_settings() {
        Ok(s) => s,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let trackers: Vec<TrackerPublic> = match state.db.list_trackers() {
        Ok(t) => t.iter().map(TrackerPublic::from).collect(),
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let channels: Vec<_> = match state.db.list_channels() {
        Ok(c) => c.iter().map(|ch| ch.to_public()).collect(),
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let mut notifications = state
        .db
        .list_notification_history(None, None, 50)
        .unwrap_or_default();
    for n in &mut notifications {
        if !is_safe_http_url(&n.url) {
            n.url.clear();
        }
    }

    Json(json!({
        "settings": settings,
        "trackers": trackers,
        "channels": channels,
        "notifications": notifications,
    }))
    .into_response()
}

async fn get_settings(State(state): State<AppState>) -> Response {
    match state.db.get_settings() {
        Ok(s) => Json(s).into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_settings(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<GlobalSettings>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    if payload.default_check_interval_secs < 60 {
        return err_resp(
            StatusCode::BAD_REQUEST,
            "Default check interval must be at least 60 seconds",
        );
    }
    if !(0.0..=300.0).contains(&payload.request_delay_secs) || payload.request_delay_secs.is_nan() {
        return err_resp(
            StatusCode::BAD_REQUEST,
            "Request delay must be between 0 and 300 seconds",
        );
    }
    if !(5.0..=300.0).contains(&payload.http_timeout_secs) || payload.http_timeout_secs.is_nan() {
        return err_resp(
            StatusCode::BAD_REQUEST,
            "HTTP timeout must be between 5 and 300 seconds",
        );
    }
    if !(256..=4096).contains(&payload.discord_embed_limit) {
        return err_resp(
            StatusCode::BAD_REQUEST,
            "Discord embed limit must be between 256 and 4096",
        );
    }

    match state.db.update_settings(&payload) {
        Ok(s) => {
            state.scheduler.reload().await;
            Json(s).into_response()
        }
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_presets() -> Json<&'static [crate::models::TrackerPreset]> {
    Json(TRACKER_PRESETS)
}

async fn list_trackers(State(state): State<AppState>) -> Response {
    match state.db.list_trackers() {
        Ok(t) => {
            let public: Vec<TrackerPublic> = t.iter().map(TrackerPublic::from).collect();
            Json(public).into_response()
        }
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn create_tracker(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<CreateTrackerRequest>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let Some(preset) = find_preset(&payload.kind) else {
        return err_resp(
            StatusCode::BAD_REQUEST,
            format!("Unknown tracker kind: {}", payload.kind),
        );
    };
    let name = payload.name.trim();
    if name.is_empty() || name.len() > 100 {
        return err_resp(
            StatusCode::BAD_REQUEST,
            "Tracker name must be between 1 and 100 characters",
        );
    }
    if payload.ignore_patterns.len() > 200 {
        return err_resp(StatusCode::BAD_REQUEST, "Too many ignore patterns (max 200)");
    }

    let resolved_url = payload
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(preset.default_base_url)
        .to_string();

    if let Err(e) =
        validate_outbound_url_with_dns(&resolved_url, state.allow_private_networks, false).await
    {
        return err_resp(StatusCode::BAD_REQUEST, format!("Invalid base URL: {e}"));
    }

    match state.db.create_tracker(payload, resolved_url) {
        Ok(t) => {
            state.scheduler.reload().await;
            (StatusCode::CREATED, Json(t.to_public())).into_response()
        }
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_tracker(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateTrackerRequest>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    if let Some(ref name) = payload.name {
        let t = name.trim();
        if t.is_empty() || t.len() > 100 {
            return err_resp(
                StatusCode::BAD_REQUEST,
                "Tracker name must be between 1 and 100 characters",
            );
        }
    }
    if let Some(ref kind) = payload.kind
        && find_preset(kind).is_none()
    {
        return err_resp(
            StatusCode::BAD_REQUEST,
            format!("Unknown tracker kind: {kind}"),
        );
    }
    if let Some(ref url) = payload.base_url
        && let Err(e) =
            validate_outbound_url_with_dns(url, state.allow_private_networks, false).await
    {
        return err_resp(StatusCode::BAD_REQUEST, format!("Invalid base URL: {e}"));
    }
    if let Some(ref patterns) = payload.ignore_patterns
        && patterns.len() > 200
    {
        return err_resp(StatusCode::BAD_REQUEST, "Too many ignore patterns (max 200)");
    }

    match state.db.update_tracker(&id, payload) {
        Ok(Some(t)) => {
            state.scheduler.reload().await;
            let public = t.to_public();
            let _ = state.event_tx.send(UiEvent::TrackerUpdated(public.clone()));
            Json(public).into_response()
        }
        Ok(None) => err_resp(StatusCode::NOT_FOUND, "Tracker not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn delete_tracker(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    match state.db.delete_tracker(&id) {
        Ok(true) => {
            state.scheduler.reload().await;
            Json(json!({ "deleted": true })).into_response()
        }
        Ok(false) => err_resp(StatusCode::NOT_FOUND, "Tracker not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn run_tracker_now(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    match state.db.get_tracker(&id) {
        Ok(Some(_)) => {
            state.scheduler.run_now(id).await;
            Json(json!({ "queued": true })).into_response()
        }
        Ok(None) => err_resp(StatusCode::NOT_FOUND, "Tracker not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn list_channels(State(state): State<AppState>) -> Response {
    match state.db.list_channels() {
        Ok(c) => {
            let public: Vec<_> = c.iter().map(|ch| ch.to_public()).collect();
            Json(public).into_response()
        }
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn create_channel(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<CreateChannelRequest>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let name = payload.name.trim();
    if name.is_empty() || name.len() > 100 {
        return err_resp(
            StatusCode::BAD_REQUEST,
            "Channel name must be between 1 and 100 characters",
        );
    }
    if let Err(e) =
        validate_channel_config_urls(payload.kind, &payload.config, state.allow_private_networks)
            .await
    {
        return err_resp(StatusCode::BAD_REQUEST, format!("Invalid channel URL: {e}"));
    }

    match state.db.create_channel(payload) {
        Ok(c) => (StatusCode::CREATED, Json(c.to_public())).into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_channel(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateChannelRequest>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    if let Some(ref name) = payload.name {
        let t = name.trim();
        if t.is_empty() || t.len() > 100 {
            return err_resp(
                StatusCode::BAD_REQUEST,
                "Channel name must be between 1 and 100 characters",
            );
        }
    }
    let existing = match state.db.get_channel(&id) {
        Ok(Some(c)) => c,
        Ok(None) => return err_resp(StatusCode::NOT_FOUND, "Channel not found"),
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if let Some(ref cfg) = payload.config {
        let mut merged = cfg.clone();
        if let (Some(new_obj), Some(old_obj)) = (merged.as_object_mut(), existing.config.as_object())
        {
            for key in ["webhook_url", "bot_token", "token"] {
                let is_blank = new_obj
                    .get(key)
                    .and_then(|v| v.as_str())
                    .is_none_or(|s| s.trim().is_empty());
                if is_blank && let Some(old_val) = old_obj.get(key) {
                    new_obj.insert(key.to_string(), old_val.clone());
                }
            }
        }
        if let Err(e) =
            validate_channel_config_urls(existing.kind, &merged, state.allow_private_networks).await
        {
            return err_resp(StatusCode::BAD_REQUEST, format!("Invalid channel URL: {e}"));
        }
    }

    match state.db.update_channel(&id, payload) {
        Ok(Some(c)) => Json(c.to_public()).into_response(),
        Ok(None) => err_resp(StatusCode::NOT_FOUND, "Channel not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn delete_channel(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    match state.db.delete_channel(&id) {
        Ok(true) => Json(json!({ "deleted": true })).into_response(),
        Ok(false) => err_resp(StatusCode::NOT_FOUND, "Channel not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn test_channel(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Response {
    if let Some(resp) = require_admin(&auth) {
        return resp;
    }
    let ch = match state.db.get_channel(&id) {
        Ok(Some(c)) => c,
        Ok(None) => return err_resp(StatusCode::NOT_FOUND, "Channel not found"),
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let settings = state.db.get_settings().unwrap_or_default();
    let item = NotificationItem {
        item_type: ItemType::Notification,
        id: "test-notification".into(),
        title: Some("PTNotifier Test".into()),
        subject: Some("Channel test notification".into()),
        sender: Some("PTNotifier".into()),
        body: Some("If you can read this, your notification channel is configured correctly!".into()),
        date: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        url: "https://github.com/wastaken7/PTNotifier".into(),
        is_staff: false,
        favicon: None,
        mark_read_url: None,
    };

    match notify::send_to_channel(
        &state.notify_client,
        &ch,
        &settings,
        "PTNotifier",
        "https://github.com",
        &item,
    )
    .await
    {
        Ok(()) => Json(json!({ "sent": true })).into_response(),
        Err(e) => err_resp(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

#[derive(Deserialize)]
struct HistoryQuery {
    tracker_id: Option<String>,
    item_type: Option<String>,
    limit: Option<usize>,
    token: Option<String>,
}

async fn list_notifications(
    State(state): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    match state.db.list_notification_history(
        q.tracker_id.as_deref().filter(|s| !s.is_empty()),
        q.item_type.as_deref().filter(|s| !s.is_empty()),
        q.limit.unwrap_or(100),
    ) {
        Ok(mut list) => {
            for n in &mut list {
                if !is_safe_http_url(&n.url) {
                    n.url.clear();
                }
            }
            Json(list).into_response()
        }
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn sanitize_host_header(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return None;
    }
    if trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
    {
        Some(trimmed)
    } else {
        None
    }
}

async fn serve_rss(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<HistoryQuery>,
) -> Response {
    let client_ip = extract_client_ip(&headers, peer.map(|c| c.0.0), state.trust_proxy);
    if let Err(retry) = state.auth_limiter.check_ip_rate(client_ip, RateBucket::Api) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": format!("Too many requests. Retry in {retry}s.") })),
        )
            .into_response();
    }

    let provided_token = q
        .token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| extract_bearer_token(&headers));

    let authorized = if let Some(raw) = provided_token {
        matches!(state.db.verify_rss_token(&sha256_hex(&raw)), Ok(true))
    } else if let Some(sess_tok) = extract_session_token(&headers) {
        matches!(
            state.db.get_valid_session(&sha256_hex(&sess_tok)),
            Ok(Some(_))
        )
    } else {
        false
    };

    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer realm=\"PTNotifier RSS\"")],
            Json(json!({ "error": "Authentication or valid RSS token required" })),
        )
            .into_response();
    }

    let tracker_id = q.tracker_id.as_deref().filter(|s| !s.is_empty());
    let item_type = q.item_type.as_deref().filter(|s| !s.is_empty());

    let entries = match state.db.list_notification_history(
        tracker_id,
        item_type,
        q.limit.unwrap_or(100),
    ) {
        Ok(list) => list,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let proto = if state.trust_proxy
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.split(',').next().is_some_and(|p| p.trim().eq_ignore_ascii_case("https")))
    {
        "https"
    } else {
        "http"
    };

    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(sanitize_host_header);

    let clean_path = uri.path();
    let (base_url, self_url) = match host {
        Some(h) => {
            let base = format!("{proto}://{h}");
            (base.clone(), format!("{base}{clean_path}"))
        }
        None => (String::from("/"), clean_path.to_string()),
    };

    let xml = rss::build_rss_feed(&entries, &base_url, &self_url, item_type);
    (
        [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
        xml,
    )
        .into_response()
}

async fn list_logs(State(state): State<AppState>) -> Response {
    Json(state.log_broadcaster.recent()).into_response()
}

async fn sse_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|res| match res {
        Ok(ev) => {
            let data = serde_json::to_string(&ev).unwrap_or_default();
            Some(Ok(Event::default().data(data)))
        }
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChannelKind;
    use axum::body::Body;
    use axum::http::Request;
    use tokio::sync::mpsc;
    use tower::ServiceExt;

    fn test_app(setup_token: Option<&str>) -> Router {
        let db = Database::open_in_memory().unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let (event_tx, _) = broadcast::channel(16);
        let log_broadcaster = LogBroadcaster::new(event_tx.clone());
        let state = AppState {
            db,
            scheduler: SchedulerHandle::new_for_test(tx),
            event_tx,
            log_broadcaster,
            notify_client: Client::new(),
            auth_limiter: AuthRateLimiter::new(),
            setup_token_hash: Arc::new(Mutex::new(setup_token.map(sha256_hex))),
            setup_token_required: setup_token.is_some(),
            trust_proxy: false,
            allow_private_networks: false,
        };
        router(state)
    }

    #[tokio::test]
    async fn unauthenticated_requests_are_blocked_and_security_headers_are_set() {
        let app = test_app(None);
        for path in [
            "/api/overview",
            "/api/trackers",
            "/api/channels",
            "/api/settings",
            "/api/logs",
            "/rss",
            "/api/rss",
        ] {
            let resp = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "path {path} must require auth");
            assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");
            assert_eq!(
                resp.headers().get("x-content-type-options").unwrap(),
                "nosniff"
            );
            assert!(resp.headers().get("content-security-policy").is_some());
        }
    }

    #[tokio::test]
    async fn full_auth_csrf_secret_masking_ssrf_and_rss_token_flow() {
        let app = test_app(Some("setup-secret-123"));

        let bad_setup = app
            .clone()
            .oneshot(
                Request::post("/api/auth/setup")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","password":"very-strong-password-1","setup_token":"wrong"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_setup.status(), StatusCode::FORBIDDEN);

        let ok_setup = app
            .clone()
            .oneshot(
                Request::post("/api/auth/setup")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","password":"very-strong-password-1","setup_token":"setup-secret-123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_setup.status(), StatusCode::CREATED);
        let set_cookie = ok_setup
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        let cookie_pair = set_cookie.split(';').next().unwrap().to_string();

        let body_bytes = axum::body::to_bytes(ok_setup.into_body(), 1024 * 64)
            .await
            .unwrap();
        let setup_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let csrf = setup_json["csrf_token"].as_str().unwrap().to_string();

        let no_csrf = app
            .clone()
            .oneshot(
                Request::post("/api/trackers")
                    .header("cookie", &cookie_pair)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"Test","kind":"unit3d","base_url":"https://tracker.example","cookies_raw":"Cookie: secret=1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);

        let ssrf = app
            .clone()
            .oneshot(
                Request::post("/api/trackers")
                    .header("cookie", &cookie_pair)
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"Bad","kind":"unit3d","base_url":"http://169.254.169.254/latest/meta-data/","cookies_raw":"Cookie: a=1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ssrf.status(), StatusCode::BAD_REQUEST);

        let created = app
            .clone()
            .oneshot(
                Request::post("/api/trackers")
                    .header("cookie", &cookie_pair)
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"Valid","kind":"unit3d","base_url":"https://tracker.example","cookies_raw":"Cookie: pass=topsecret","api_token":"tok123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let created_text = String::from_utf8(
            axum::body::to_bytes(created.into_body(), 1024 * 64)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!created_text.contains("topsecret"));
        assert!(!created_text.contains("tok123"));
        assert!(created_text.contains("\"cookies_configured\":true"));
        assert!(created_text.contains("\"api_token_configured\":true"));

        let ch_req = CreateChannelRequest {
            name: "Discord".into(),
            kind: ChannelKind::Discord,
            enabled: true,
            config: serde_json::json!({"webhook_url": "https://discord.com/api/webhooks/123/secret"}),
            notify_notifications: true,
            notify_pms: true,
            notify_errors: false,
        };
        let ch_resp = app
            .clone()
            .oneshot(
                Request::post("/api/channels")
                    .header("cookie", &cookie_pair)
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&ch_req).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ch_resp.status(), StatusCode::CREATED);
        let ch_text = String::from_utf8(
            axum::body::to_bytes(ch_resp.into_body(), 1024 * 64)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!ch_text.contains("discord.com/api/webhooks/123/secret"));

        let rss_create = app
            .clone()
            .oneshot(
                Request::post("/api/auth/rss-tokens")
                    .header("cookie", &cookie_pair)
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"label":"Reader"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rss_create.status(), StatusCode::CREATED);
        let rss_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(rss_create.into_body(), 1024 * 64)
                .await
                .unwrap(),
        )
        .unwrap();
        let rss_tok = rss_json["token"].as_str().unwrap();

        let rss_feed = app
            .clone()
            .oneshot(
                Request::get(format!("/rss?token={rss_tok}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rss_feed.status(), StatusCode::OK);
        let rss_xml = String::from_utf8(
            axum::body::to_bytes(rss_feed.into_body(), 1024 * 64)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!rss_xml.contains(rss_tok));
    }
}
