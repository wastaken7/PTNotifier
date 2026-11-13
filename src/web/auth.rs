use crate::models::{
    AuthStatusResponse, ChangePasswordRequest, CreateRssTokenRequest, CreateUserRequest,
    LoginRequest, SessionRecord, SetupRequest, UserPublic, UserRecord, UserRole,
};
use crate::security::{
    RateBucket, constant_time_eq, dummy_verify_password, generate_token, hash_password, sha256_hex,
    validate_password, validate_username, verify_password,
};
use crate::web::{AppState, err_resp};
use axum::extract::{ConnectInfo, Extension, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tracing::{info, warn};

pub const SESSION_COOKIE_NAME: &str = "ptn_session";
const SESSION_TTL_DAYS: i64 = 7;

#[derive(Clone)]
pub struct AuthContext {
    pub user: UserRecord,
    pub session: SessionRecord,
}

pub fn extract_client_ip(headers: &HeaderMap, peer: Option<SocketAddr>, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            for part in xff.split(',') {
                if let Ok(ip) = part.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
        if let Some(xri) = headers.get("x-real-ip").and_then(|v| v.to_str().ok())
            && let Ok(ip) = xri.trim().parse::<IpAddr>()
        {
            return ip;
        }
    }
    peer.map(|s| s.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

pub fn is_https_request(headers: &HeaderMap, trust_proxy: bool) -> bool {
    if std::env::var("PTN_COOKIE_SECURE")
        .ok()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
    {
        return true;
    }
    if trust_proxy
        && let Some(proto) = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
        && proto.split(',').next().is_some_and(|p| p.trim().eq_ignore_ascii_case("https"))
    {
        return true;
    }
    false
}

pub fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_hdr = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie_hdr.split(';') {
        let trimmed = pair.trim();
        if let Some((name, val)) = trimmed.split_once('=')
            && name.trim() == SESSION_COOKIE_NAME
        {
            let token = val.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = auth.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() {
        return Some(token.trim().to_string());
    }
    None
}

fn build_session_cookie(token: &str, max_age_secs: i64, secure: bool) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn verify_same_origin(headers: &HeaderMap, trust_proxy: bool) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Ok(origin_url) = url::Url::parse(origin) else {
        return false;
    };
    let Some(origin_host) = origin_url.host_str() else {
        return false;
    };
    let expected_host = if trust_proxy {
        headers
            .get("x-forwarded-host")
            .or_else(|| headers.get(header::HOST))
            .and_then(|v| v.to_str().ok())
    } else {
        headers.get(header::HOST).and_then(|v| v.to_str().ok())
    };
    let Some(host_hdr) = expected_host else {
        return false;
    };
    let host_only = host_hdr
        .split(',')
        .next()
        .unwrap_or("")
        .trim();
    let origin_authority = match origin_url.port() {
        Some(p) => format!("{origin_host}:{p}"),
        None => origin_host.to_string(),
    };
    origin_authority.eq_ignore_ascii_case(host_only) || origin_host.eq_ignore_ascii_case(host_only)
}

fn is_mutating_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

pub async fn security_headers_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let is_https = is_https_request(req.headers(), state.trust_proxy);
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();

    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers.insert(
        header::HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
        ),
    );
    if path.starts_with("/api/") || path == "/rss" {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate"),
        );
    }
    if is_https {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    resp
}

pub async fn require_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0);
    let client_ip = extract_client_ip(req.headers(), peer, state.trust_proxy);

    if let Err(retry_after) = state.auth_limiter.check_ip_rate(client_ip, RateBucket::Api) {
        return rate_limited_resp(retry_after);
    }

    let Some(raw_token) = extract_session_token(req.headers()) else {
        return err_resp(StatusCode::UNAUTHORIZED, "Authentication required");
    };
    let token_hash = sha256_hex(&raw_token);
    let session_user = match state.db.get_valid_session(&token_hash) {
        Ok(Some(su)) => su,
        Ok(None) => return err_resp(StatusCode::UNAUTHORIZED, "Session expired or invalid"),
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if is_mutating_method(req.method()) {
        if !verify_same_origin(req.headers(), state.trust_proxy) {
            return err_resp(StatusCode::FORBIDDEN, "Cross-origin request rejected");
        }
        let provided_csrf = req
            .headers()
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !constant_time_eq(provided_csrf, &session_user.0.csrf_token) {
            return err_resp(StatusCode::FORBIDDEN, "Invalid or missing CSRF token");
        }
    }

    req.extensions_mut().insert(AuthContext {
        session: session_user.0,
        user: session_user.1,
    });
    next.run(req).await
}

fn rate_limited_resp(retry_after: u64) -> Response {
    let mut resp = err_resp(
        StatusCode::TOO_MANY_REQUESTS,
        format!("Too many requests. Try again in {retry_after}s."),
    );
    if let Ok(val) = HeaderValue::from_str(&retry_after.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, val);
    }
    resp
}

pub async fn auth_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user_count = match state.db.count_users() {
        Ok(c) => c,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if user_count == 0 {
        return Json(AuthStatusResponse {
            authenticated: false,
            setup_required: true,
            setup_token_required: state.setup_token_required,
            user: None,
            csrf_token: None,
        })
        .into_response();
    }

    if let Some(raw_token) = extract_session_token(&headers) {
        let token_hash = sha256_hex(&raw_token);
        if let Ok(Some((sess, user))) = state.db.get_valid_session(&token_hash) {
            return Json(AuthStatusResponse {
                authenticated: true,
                setup_required: false,
                setup_token_required: false,
                user: Some(UserPublic::from(&user)),
                csrf_token: Some(sess.csrf_token),
            })
            .into_response();
        }
    }

    Json(AuthStatusResponse {
        authenticated: false,
        setup_required: false,
        setup_token_required: false,
        user: None,
        csrf_token: None,
    })
    .into_response()
}

pub async fn setup_admin(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(payload): Json<SetupRequest>,
) -> Response {
    let client_ip = extract_client_ip(&headers, peer.map(|c| c.0.0), state.trust_proxy);
    if let Err(retry) = state.auth_limiter.check_ip_rate(client_ip, RateBucket::Auth) {
        return rate_limited_resp(retry);
    }

    if !verify_same_origin(&headers, state.trust_proxy) {
        return err_resp(StatusCode::FORBIDDEN, "Cross-origin setup rejected");
    }

    let count = match state.db.count_users() {
        Ok(c) => c,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if count > 0 {
        return err_resp(StatusCode::CONFLICT, "Initial setup is already complete");
    }

    if state.setup_token_required {
        let provided = payload.setup_token.as_deref().unwrap_or("").trim();
        let expected_hash = state.setup_token_hash.lock().unwrap().clone();
        let Some(expected) = expected_hash else {
            return err_resp(StatusCode::FORBIDDEN, "Setup token is unavailable");
        };
        if provided.is_empty() || !constant_time_eq(&sha256_hex(provided), &expected) {
            warn!(ip = %client_ip, "Rejected initial setup attempt with invalid setup token");
            return err_resp(StatusCode::FORBIDDEN, "Invalid setup token");
        }
    }

    let username = match validate_username(&payload.username) {
        Ok(u) => u,
        Err(e) => return err_resp(StatusCode::BAD_REQUEST, e.to_string()),
    };
    if let Err(e) = validate_password(&payload.password) {
        return err_resp(StatusCode::BAD_REQUEST, e.to_string());
    }

    let pw_hash = match hash_password(&payload.password) {
        Ok(h) => h,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let user = match state.db.create_user(&username, &pw_hash, UserRole::Admin) {
        Ok(u) => u,
        Err(e) => return err_resp(StatusCode::CONFLICT, e.to_string()),
    };
    *state.setup_token_hash.lock().unwrap() = None;
    info!(username = %user.username, ip = %client_ip, "Initial administrator account created");

    create_session_response(&state, &headers, client_ip, &user, StatusCode::CREATED)
}

pub async fn login(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let client_ip = extract_client_ip(&headers, peer.map(|c| c.0.0), state.trust_proxy);
    if let Err(retry) = state.auth_limiter.check_ip_rate(client_ip, RateBucket::Auth) {
        return rate_limited_resp(retry);
    }

    if !verify_same_origin(&headers, state.trust_proxy) {
        return err_resp(StatusCode::FORBIDDEN, "Cross-origin login rejected");
    }

    let username = payload.username.trim();
    if username.is_empty() || payload.password.is_empty() || payload.password.len() > 128 {
        return err_resp(StatusCode::BAD_REQUEST, "Username and password are required");
    }

    if let Err(retry) = state.auth_limiter.check_account_lockout(username) {
        warn!(username = %username, ip = %client_ip, "Blocked login during account lockout");
        return rate_limited_resp(retry);
    }

    let user_opt = match state.db.get_user_by_username(username) {
        Ok(u) => u,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let Some(user) = user_opt else {
        dummy_verify_password(&payload.password);
        state.auth_limiter.record_login_failure(username);
        warn!(username = %username, ip = %client_ip, "Failed login attempt (unknown user)");
        return err_resp(StatusCode::UNAUTHORIZED, "Invalid username or password");
    };

    if !verify_password(&payload.password, &user.password_hash) {
        state.auth_limiter.record_login_failure(&user.username);
        warn!(username = %user.username, ip = %client_ip, "Failed login attempt (bad password)");
        return err_resp(StatusCode::UNAUTHORIZED, "Invalid username or password");
    }

    state.auth_limiter.record_login_success(&user.username);
    let _ = state.db.update_user_last_login(&user.id);
    info!(username = %user.username, ip = %client_ip, "User signed in");

    create_session_response(&state, &headers, client_ip, &user, StatusCode::OK)
}

fn create_session_response(
    state: &AppState,
    headers: &HeaderMap,
    client_ip: IpAddr,
    user: &UserRecord,
    status: StatusCode,
) -> Response {
    let raw_token = generate_token(32);
    let token_hash = sha256_hex(&raw_token);
    let csrf_token = generate_token(24);
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("Unknown");
    let expires_at = (Utc::now() + ChronoDuration::days(SESSION_TTL_DAYS)).to_rfc3339();

    if let Err(e) = state.db.create_session(
        &token_hash,
        &user.id,
        user.token_version,
        &csrf_token,
        ua,
        &client_ip.to_string(),
        &expires_at,
    ) {
        return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    let secure = is_https_request(headers, state.trust_proxy);
    let cookie_val = build_session_cookie(&raw_token, SESSION_TTL_DAYS * 86_400, secure);
    let mut resp = (
        status,
        Json(json!({
            "authenticated": true,
            "user": UserPublic::from(user),
            "csrf_token": csrf_token,
        })),
    )
        .into_response();
    if let Ok(hdr) = HeaderValue::from_str(&cookie_val) {
        resp.headers_mut().insert(header::SET_COOKIE, hdr);
    }
    resp
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !verify_same_origin(&headers, state.trust_proxy) {
        return err_resp(StatusCode::FORBIDDEN, "Cross-origin logout rejected");
    }
    if let Some(raw_token) = extract_session_token(&headers) {
        let token_hash = sha256_hex(&raw_token);
        if let Ok(Some((sess, user))) = state.db.get_valid_session(&token_hash) {
            let provided_csrf = headers
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !constant_time_eq(provided_csrf, &sess.csrf_token) {
                return err_resp(StatusCode::FORBIDDEN, "Invalid or missing CSRF token");
            }
            info!(username = %user.username, "User signed out");
        }
        let _ = state.db.delete_session(&token_hash);
    }
    let secure = is_https_request(&headers, state.trust_proxy);
    let clear_cookie = build_session_cookie("", 0, secure);
    let mut resp = Json(json!({ "logged_out": true })).into_response();
    if let Ok(hdr) = HeaderValue::from_str(&clear_cookie) {
        resp.headers_mut().insert(header::SET_COOKIE, hdr);
    }
    resp
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Response {
    if !verify_password(&payload.current_password, &auth.user.password_hash) {
        return err_resp(StatusCode::UNAUTHORIZED, "Current password is incorrect");
    }
    if let Err(e) = validate_password(&payload.new_password) {
        return err_resp(StatusCode::BAD_REQUEST, e.to_string());
    }
    let new_hash = match hash_password(&payload.new_password) {
        Ok(h) => h,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    match state
        .db
        .update_user_password(&auth.user.id, &new_hash, Some(&auth.session.token_hash))
    {
        Ok(true) => {
            info!(username = %auth.user.username, "Password updated and other sessions revoked");
            Json(json!({ "updated": true })).into_response()
        }
        Ok(false) => err_resp(StatusCode::NOT_FOUND, "User not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn list_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Response {
    match state
        .db
        .list_user_sessions(&auth.user.id, &auth.session.token_hash)
    {
        Ok(list) => Json(list).into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn revoke_session(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(prefix): Path<String>,
) -> Response {
    match state.db.delete_session_by_prefix(&auth.user.id, &prefix) {
        Ok(true) => Json(json!({ "revoked": true })).into_response(),
        Ok(false) => err_resp(StatusCode::NOT_FOUND, "Session not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn revoke_other_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Response {
    match state
        .db
        .delete_other_sessions(&auth.user.id, &auth.session.token_hash)
    {
        Ok(count) => Json(json!({ "revoked_count": count })).into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn list_rss_tokens(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Response {
    match state.db.list_rss_tokens(&auth.user.id) {
        Ok(tokens) => Json(tokens).into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn create_rss_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<CreateRssTokenRequest>,
) -> Response {
    let raw_token = format!("ptnrss_{}", generate_token(24));
    let token_hash = sha256_hex(&raw_token);
    let prefix: String = raw_token.chars().take(12).collect();
    match state
        .db
        .create_rss_token(&auth.user.id, &payload.label, &token_hash, &prefix)
    {
        Ok(meta) => (
            StatusCode::CREATED,
            Json(json!({
                "token": raw_token,
                "meta": meta,
            })),
        )
            .into_response(),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn delete_rss_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Response {
    match state.db.delete_rss_token(&auth.user.id, &id) {
        Ok(true) => Json(json!({ "deleted": true })).into_response(),
        Ok(false) => err_resp(StatusCode::NOT_FOUND, "RSS token not found"),
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Response {
    if auth.user.role != UserRole::Admin {
        return err_resp(StatusCode::FORBIDDEN, "Administrator privileges required");
    }
    match state.db.list_users() {
        Ok(users) => {
            let public: Vec<UserPublic> = users.iter().map(UserPublic::from).collect();
            Json(public).into_response()
        }
        Err(e) => err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn create_user(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<CreateUserRequest>,
) -> Response {
    if auth.user.role != UserRole::Admin {
        return err_resp(StatusCode::FORBIDDEN, "Administrator privileges required");
    }
    let username = match validate_username(&payload.username) {
        Ok(u) => u,
        Err(e) => return err_resp(StatusCode::BAD_REQUEST, e.to_string()),
    };
    if let Err(e) = validate_password(&payload.password) {
        return err_resp(StatusCode::BAD_REQUEST, e.to_string());
    }
    let pw_hash = match hash_password(&payload.password) {
        Ok(h) => h,
        Err(e) => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    match state.db.create_user(&username, &pw_hash, payload.role) {
        Ok(u) => {
            info!(created_user = %u.username, by = %auth.user.username, "Created user account");
            (StatusCode::CREATED, Json(UserPublic::from(&u))).into_response()
        }
        Err(e) => err_resp(StatusCode::CONFLICT, e.to_string()),
    }
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Response {
    if auth.user.role != UserRole::Admin {
        return err_resp(StatusCode::FORBIDDEN, "Administrator privileges required");
    }
    if auth.user.id == id {
        return err_resp(StatusCode::BAD_REQUEST, "Cannot delete your own active account");
    }
    match state.db.delete_user(&id) {
        Ok(true) => {
            info!(deleted_user_id = %id, by = %auth.user.username, "Deleted user account");
            Json(json!({ "deleted": true })).into_response()
        }
        Ok(false) => err_resp(StatusCode::NOT_FOUND, "User not found"),
        Err(e) => err_resp(StatusCode::BAD_REQUEST, e.to_string()),
    }
}
