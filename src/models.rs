use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettings {
    pub default_check_interval_secs: u64,
    pub request_delay_secs: f64,
    pub http_timeout_secs: f64,
    pub mark_as_read: bool,
    pub send_error_notifications: bool,
    pub discord_embed_limit: usize,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            default_check_interval_secs: 1800,
            request_delay_secs: 5.0,
            http_timeout_secs: 30.0,
            mark_as_read: false,
            send_error_notifications: false,
            discord_embed_limit: 4096,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerStatus {
    Pending,
    Running,
    Ok,
    CookieExpired,
    Error,
    Paused,
}

impl TrackerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Ok => "ok",
            Self::CookieExpired => "cookie_expired",
            Self::Error => "error",
            Self::Paused => "paused",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "ok" => Self::Ok,
            "cookie_expired" => Self::CookieExpired,
            "error" => Self::Error,
            "paused" => Self::Paused,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackerState {
    #[serde(default)]
    pub processed_ids: Vec<String>,
    #[serde(default)]
    pub notifications_url: Option<String>,
    #[serde(default)]
    pub messages_url: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub initialized: bool,
    /// Items whose delivery failed on one or more channels, keyed by item ID.
    /// They stay unprocessed and are retried for the listed channels on the next check.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub pending_deliveries: std::collections::HashMap<String, PendingDelivery>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingDelivery {
    pub channel_ids: Vec<String>,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerConfig {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub enabled: bool,
    pub check_interval_secs: Option<u64>,
    pub mark_as_read: Option<bool>,
    pub cookies_raw: String,
    pub api_token: String,
    pub ignore_patterns: Vec<String>,
    pub state: TrackerState,
    pub last_status: TrackerStatus,
    pub last_error: Option<String>,
    pub last_checked_at: Option<String>,
    pub next_check_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTrackerRequest {
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub check_interval_secs: Option<u64>,
    pub mark_as_read: Option<bool>,
    #[serde(default)]
    pub cookies_raw: String,
    #[serde(default)]
    pub api_token: String,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTrackerRequest {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    /// Missing field = keep current value; explicit `null` = clear the override.
    #[serde(default, deserialize_with = "double_option")]
    pub check_interval_secs: Option<Option<u64>>,
    /// Missing field = keep current value; explicit `null` = clear the override.
    #[serde(default, deserialize_with = "double_option")]
    pub mark_as_read: Option<Option<bool>>,
    pub cookies_raw: Option<String>,
    pub api_token: Option<String>,
    #[serde(default)]
    pub clear_cookies: bool,
    #[serde(default)]
    pub clear_api_token: bool,
    pub ignore_patterns: Option<Vec<String>>,
}

/// Public API/SSE view of a tracker that omits raw cookies and API tokens.
#[derive(Debug, Clone, Serialize)]
pub struct TrackerPublic {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub enabled: bool,
    pub check_interval_secs: Option<u64>,
    pub mark_as_read: Option<bool>,
    pub cookies_configured: bool,
    pub api_token_configured: bool,
    pub ignore_patterns: Vec<String>,
    pub state: TrackerState,
    pub last_status: TrackerStatus,
    pub last_error: Option<String>,
    pub last_checked_at: Option<String>,
    pub next_check_at: Option<String>,
    pub created_at: String,
}

impl From<&TrackerConfig> for TrackerPublic {
    fn from(t: &TrackerConfig) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            kind: t.kind.clone(),
            base_url: t.base_url.clone(),
            enabled: t.enabled,
            check_interval_secs: t.check_interval_secs,
            mark_as_read: t.mark_as_read,
            cookies_configured: !t.cookies_raw.trim().is_empty(),
            api_token_configured: !t.api_token.trim().is_empty(),
            ignore_patterns: t.ignore_patterns.clone(),
            state: t.state.clone(),
            last_status: t.last_status,
            last_error: t.last_error.clone(),
            last_checked_at: t.last_checked_at.clone(),
            next_check_at: t.next_check_at.clone(),
            created_at: t.created_at.clone(),
        }
    }
}

impl TrackerConfig {
    pub fn to_public(&self) -> TrackerPublic {
        TrackerPublic::from(self)
    }
}

fn default_true() -> bool {
    true
}

/// Deserializes a present field (including `null`) as `Some(..)`; with `#[serde(default)]`
/// an absent field stays `None`. Plain `Option<Option<T>>` maps `null` to `None`, which would
/// make "clear this override" indistinguishable from "leave it unchanged".
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthRequirement {
    Cookies,
    ApiToken,
    CookiesAndApiToken,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackerPreset {
    pub kind: &'static str,
    pub label: &'static str,
    pub default_base_url: &'static str,
    pub custom_base_url: bool,
    pub auth: AuthRequirement,
    pub min_interval_secs: u64,
    pub notes: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Discord,
    Telegram,
    Gotify,
    Ntfy,
}

impl ChannelKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Discord => "discord",
            Self::Telegram => "telegram",
            Self::Gotify => "gotify",
            Self::Ntfy => "ntfy",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "discord" => Some(Self::Discord),
            "telegram" => Some(Self::Telegram),
            "gotify" => Some(Self::Gotify),
            "ntfy" => Some(Self::Ntfy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub notify_notifications: bool,
    pub notify_pms: bool,
    pub notify_errors: bool,
    pub created_at: String,
}

/// Public API view of a notification channel with secret fields redacted.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelPublic {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub secret_fields: Vec<String>,
    pub notify_notifications: bool,
    pub notify_pms: bool,
    pub notify_errors: bool,
    pub created_at: String,
}

impl ChannelConfig {
    pub fn to_public(&self) -> ChannelPublic {
        let mut sanitized = self.config.clone();
        let mut secret_fields = Vec::new();
        if let Some(obj) = sanitized.as_object_mut() {
            for key in ["webhook_url", "bot_token", "token"] {
                if let Some(val) = obj.get(key).and_then(|v| v.as_str())
                    && !val.trim().is_empty()
                {
                    secret_fields.push(key.to_string());
                    obj.insert(key.to_string(), serde_json::Value::String(String::new()));
                }
            }
        }
        ChannelPublic {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            enabled: self.enabled,
            config: sanitized,
            secret_fields,
            notify_notifications: self.notify_notifications,
            notify_pms: self.notify_pms,
            notify_errors: self.notify_errors,
            created_at: self.created_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub kind: ChannelKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub config: serde_json::Value,
    #[serde(default = "default_true")]
    pub notify_notifications: bool,
    #[serde(default = "default_true")]
    pub notify_pms: bool,
    #[serde(default)]
    pub notify_errors: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<serde_json::Value>,
    pub notify_notifications: Option<bool>,
    pub notify_pms: Option<bool>,
    pub notify_errors: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    Notification,
    Message,
    Error,
}

impl ItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Notification => "notification",
            Self::Message => "message",
            Self::Error => "error",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Notification => "Notification",
            Self::Message => "Message",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationItem {
    pub item_type: ItemType,
    pub id: String,
    pub title: Option<String>,
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub body: Option<String>,
    pub date: String,
    pub url: String,
    pub is_staff: bool,
    pub favicon: Option<String>,
    pub mark_read_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationHistoryEntry {
    pub id: i64,
    pub tracker_id: String,
    pub tracker_name: String,
    pub item_type: String,
    pub external_id: String,
    pub title: Option<String>,
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub body: Option<String>,
    pub is_staff: bool,
    pub item_date: String,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum UiEvent {
    Log(LogEvent),
    TrackerUpdated(TrackerPublic),
    NewNotification(NotificationHistoryEntry),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "admin" => Self::Admin,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: UserRole,
    pub token_version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub last_login_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserPublic {
    pub id: String,
    pub username: String,
    pub role: UserRole,
    pub created_at: String,
    pub updated_at: String,
    pub last_login_at: Option<String>,
}

impl From<&UserRecord> for UserPublic {
    fn from(u: &UserRecord) -> Self {
        Self {
            id: u.id.clone(),
            username: u.username.clone(),
            role: u.role,
            created_at: u.created_at.clone(),
            updated_at: u.updated_at.clone(),
            last_login_at: u.last_login_at.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub token_hash: String,
    pub token_version: i64,
    pub csrf_token: String,
    pub expires_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionPublic {
    pub id_prefix: String,
    pub current: bool,
    pub user_agent: String,
    pub ip_address: String,
    pub created_at: String,
    pub expires_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RssTokenPublic {
    pub id: String,
    pub label: String,
    pub token_prefix: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
    pub setup_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default = "default_user_role")]
    pub role: UserRole,
}

fn default_user_role() -> UserRole {
    UserRole::User
}

#[derive(Debug, Deserialize)]
pub struct CreateRssTokenRequest {
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub setup_required: bool,
    pub setup_token_required: bool,
    pub user: Option<UserPublic>,
    pub csrf_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_tracker_request_distinguishes_null_from_missing() {
        let missing: UpdateTrackerRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(missing.check_interval_secs, None);
        assert_eq!(missing.mark_as_read, None);

        let cleared: UpdateTrackerRequest =
            serde_json::from_str(r#"{"check_interval_secs":null,"mark_as_read":null}"#).unwrap();
        assert_eq!(cleared.check_interval_secs, Some(None));
        assert_eq!(cleared.mark_as_read, Some(None));

        let set: UpdateTrackerRequest =
            serde_json::from_str(r#"{"check_interval_secs":3600,"mark_as_read":false}"#).unwrap();
        assert_eq!(set.check_interval_secs, Some(Some(3600)));
        assert_eq!(set.mark_as_read, Some(Some(false)));
    }
}
