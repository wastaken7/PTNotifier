use anyhow::{Context, Result, anyhow, bail};
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD as B64_STD, URL_SAFE_NO_PAD};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, NONCE_LEN, UnboundKey};
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;

static ALLOW_PRIVATE_NETWORKS: AtomicBool = AtomicBool::new(false);

pub fn set_allow_private_networks(allow: bool) {
    ALLOW_PRIVATE_NETWORKS.store(allow, Ordering::Relaxed);
}

pub fn allow_private_networks() -> bool {
    ALLOW_PRIVATE_NETWORKS.load(Ordering::Relaxed)
}

pub fn safe_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("Too many redirects");
        }
        let allow_private = allow_private_networks();
        if let Err(e) = validate_outbound_url(attempt.url().as_str(), allow_private, false, None) {
            return attempt.error(format!("Blocked unsafe redirect target: {e}"));
        }
        attempt.follow()
    })
}

const ENC_PREFIX: &str = "enc:v1:";
const KEY_LEN: usize = 32;

/// AES-256-GCM symmetric cipher for encrypting sensitive credentials stored in SQLite.
#[derive(Clone)]
pub struct SecretCipher {
    key: Arc<LessSafeKey>,
}

impl SecretCipher {
    pub fn from_bytes(raw_key: &[u8; KEY_LEN]) -> Result<Self> {
        let unbound = UnboundKey::new(&AES_256_GCM, raw_key)
            .map_err(|_| anyhow!("failed to initialize AES-256-GCM key"))?;
        Ok(Self {
            key: Arc::new(LessSafeKey::new(unbound)),
        })
    }

    #[cfg(test)]
    pub fn ephemeral() -> Result<Self> {
        let mut raw = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut raw)
            .map_err(|_| anyhow!("CSPRNG failed"))?;
        Self::from_bytes(&raw)
    }

    /// Loads the master key from `PTN_MASTER_KEY` or `<db_dir>/.ptn_master.key` (creating it if absent).
    pub fn load_or_create_for_db(db_path: &Path) -> Result<Self> {
        if let Ok(env_val) = std::env::var("PTN_MASTER_KEY") {
            let trimmed = env_val.trim();
            if !trimmed.is_empty() {
                let raw = decode_master_key(trimmed)
                    .context("invalid PTN_MASTER_KEY (expected 64 hex chars or 32-byte base64)")?;
                return Self::from_bytes(&raw);
            }
        }

        let key_path = master_key_path(db_path);
        if key_path.exists() {
            let content = std::fs::read_to_string(&key_path)
                .with_context(|| format!("failed to read master key file {}", key_path.display()))?;
            let raw = decode_master_key(content.trim())
                .with_context(|| format!("invalid master key in {}", key_path.display()))?;
            return Self::from_bytes(&raw);
        }

        if let Some(parent) = key_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let mut raw = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut raw)
            .map_err(|_| anyhow!("CSPRNG failed"))?;
        let encoded = hex_encode(&raw);

        write_secret_file(&key_path, &encoded)?;
        Self::from_bytes(&raw)
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }
        let mut nonce_bytes = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| anyhow!("CSPRNG failed"))?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut buf = plaintext.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut buf)
            .map_err(|_| anyhow!("encryption failed"))?;

        let mut combined = Vec::with_capacity(NONCE_LEN + buf.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&buf);
        Ok(format!("{ENC_PREFIX}{}", URL_SAFE_NO_PAD.encode(combined)))
    }

    /// Decrypts `enc:v1:...` values. Unprefixed legacy plaintext is returned unchanged for migration.
    pub fn decrypt(&self, stored: &str) -> Result<String> {
        if stored.is_empty() {
            return Ok(String::new());
        }
        let Some(payload) = stored.strip_prefix(ENC_PREFIX) else {
            return Ok(stored.to_string());
        };
        let mut decoded = URL_SAFE_NO_PAD
            .decode(payload)
            .context("invalid encrypted payload encoding")?;
        if decoded.len() < NONCE_LEN + AES_256_GCM.tag_len() {
            bail!("encrypted payload is truncated");
        }
        let (nonce_slice, ciphertext) = decoded.split_at_mut(NONCE_LEN);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(nonce_slice);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let plaintext = self
            .key
            .open_in_place(nonce, Aad::empty(), ciphertext)
            .map_err(|_| anyhow!("decryption failed (wrong PTN_MASTER_KEY or corrupted data)"))?;
        String::from_utf8(plaintext.to_vec()).context("decrypted secret is not valid UTF-8")
    }

    pub fn is_encrypted(stored: &str) -> bool {
        stored.is_empty() || stored.starts_with(ENC_PREFIX)
    }
}

fn master_key_path(db_path: &Path) -> PathBuf {
    let dir = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    dir.join(".ptn_master.key")
}

fn write_secret_file(path: &Path, content: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("failed to create key file {}", path.display()))?;
        file.write_all(content.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("failed to create key file {}", path.display()))?;
        file.write_all(content.as_bytes())?;
    }
    Ok(())
}

fn decode_master_key(raw: &str) -> Result<[u8; KEY_LEN]> {
    if raw.len() == KEY_LEN * 2 && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut out = [0u8; KEY_LEN];
        let (chunks, _) = raw.as_bytes().as_chunks::<2>();
        for (i, chunk) in chunks.iter().enumerate() {
            let s = std::str::from_utf8(chunk)?;
            out[i] = u8::from_str_radix(s, 16)?;
        }
        return Ok(out);
    }

    let decoded = URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| B64_STD.decode(raw))
        .context("master key must be 64 hex chars or 32-byte base64")?;
    if decoded.len() != KEY_LEN {
        bail!("master key must decode to exactly 32 bytes");
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&decoded);
    Ok(out)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn generate_token(num_bytes: usize) -> String {
    let mut buf = vec![0u8; num_bytes];
    SystemRandom::new()
        .fill(&mut buf)
        .expect("system CSPRNG must succeed");
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn sha256_hex(input: &str) -> String {
    let d = digest(&SHA256, input.as_bytes());
    hex_encode(d.as_ref())
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= std::hint::black_box(x ^ y);
    }
    diff == 0
}

pub fn validate_username(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 3 || trimmed.len() > 64 {
        bail!("Username must be between 3 and 64 characters");
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        bail!("Username may only contain letters, digits, '_', '-', and '.'");
    }
    Ok(trimmed.to_string())
}

pub fn validate_password(password: &str) -> Result<()> {
    if password.len() < 10 {
        bail!("Password must be at least 10 characters long");
    }
    if password.len() > 128 {
        bail!("Password must be at most 128 characters long");
    }
    if password.chars().all(|c| c.is_whitespace()) {
        bail!("Password cannot be only whitespace");
    }
    Ok(())
}

pub fn hash_password(password: &str) -> Result<String> {
    let mut salt_bytes = [0u8; 16];
    SystemRandom::new()
        .fill(&mut salt_bytes)
        .map_err(|_| anyhow!("CSPRNG failed"))?;
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| anyhow!("salt encoding failed: {e}"))?;
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("password hashing failed: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, phc_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Runs an Argon2id check against a fixed dummy hash when the username is not found
/// so response timing stays uniform and prevents username enumeration.
pub fn dummy_verify_password(password: &str) {
    static DUMMY_HASH: OnceLock<String> = OnceLock::new();
    let dummy = DUMMY_HASH.get_or_init(|| {
        hash_password("ptnotifier-dummy-constant-time-check")
            .unwrap_or_else(|_| "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into())
    });
    let _ = verify_password(password, dummy);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateBucket {
    Auth,
    Api,
}

struct WindowEntry {
    count: u32,
    window_start: Instant,
}

struct LockoutEntry {
    failures: u32,
    locked_until: Option<Instant>,
    last_failure: Instant,
}

/// In-memory rate limiter for auth brute-force defense, account lockouts, and API abuse protection.
#[derive(Clone)]
pub struct AuthRateLimiter {
    inner: Arc<Mutex<RateLimiterState>>,
}

struct RateLimiterState {
    ip_windows: HashMap<(IpAddr, RateBucket), WindowEntry>,
    account_lockouts: HashMap<String, LockoutEntry>,
}

impl AuthRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterState {
                ip_windows: HashMap::new(),
                account_lockouts: HashMap::new(),
            })),
        }
    }

    pub fn check_ip_rate(&self, ip: IpAddr, bucket: RateBucket) -> std::result::Result<(), u64> {
        let (max_requests, window) = match bucket {
            RateBucket::Auth => (12, Duration::from_secs(60)),
            RateBucket::Api => (180, Duration::from_secs(60)),
        };
        let now = Instant::now();
        let mut state = self.inner.lock().unwrap();

        if state.ip_windows.len() > 10_000 {
            state
                .ip_windows
                .retain(|_, v| now.duration_since(v.window_start) < Duration::from_secs(120));
        }

        let entry = state.ip_windows.entry((ip, bucket)).or_insert(WindowEntry {
            count: 0,
            window_start: now,
        });

        let elapsed = now.duration_since(entry.window_start);
        if elapsed >= window {
            entry.count = 1;
            entry.window_start = now;
            return Ok(());
        }

        if entry.count >= max_requests {
            let retry = (window - elapsed).as_secs().max(1);
            return Err(retry);
        }

        entry.count += 1;
        Ok(())
    }

    pub fn check_account_lockout(&self, username: &str) -> std::result::Result<(), u64> {
        let key = username.trim().to_ascii_lowercase();
        let now = Instant::now();
        let mut state = self.inner.lock().unwrap();
        if let Some(entry) = state.account_lockouts.get_mut(&key)
            && let Some(until) = entry.locked_until
        {
            if until > now {
                return Err((until - now).as_secs().max(1));
            }
            entry.locked_until = None;
        }
        Ok(())
    }

    pub fn record_login_failure(&self, username: &str) {
        let key = username.trim().to_ascii_lowercase();
        let now = Instant::now();
        let mut state = self.inner.lock().unwrap();

        if state.account_lockouts.len() > 5_000 {
            state
                .account_lockouts
                .retain(|_, v| now.duration_since(v.last_failure) < Duration::from_secs(3600));
        }

        let entry = state.account_lockouts.entry(key).or_insert(LockoutEntry {
            failures: 0,
            locked_until: None,
            last_failure: now,
        });
        if now.duration_since(entry.last_failure) > Duration::from_secs(900) {
            entry.failures = 0;
        }
        entry.failures += 1;
        entry.last_failure = now;

        // Exponential backoff after 5 consecutive failures: 60s, 180s, 600s, 900s
        if entry.failures >= 5 {
            let lock_secs = match entry.failures {
                5..=6 => 60,
                7..=8 => 180,
                9..=11 => 600,
                _ => 900,
            };
            entry.locked_until = Some(now + Duration::from_secs(lock_secs));
        }
    }

    pub fn record_login_success(&self, username: &str) {
        let key = username.trim().to_ascii_lowercase();
        let mut state = self.inner.lock().unwrap();
        state.account_lockouts.remove(&key);
    }
}

/// Validates an outbound URL against SSRF risks (scheme, credentials, loopback, metadata, private IPs).
pub fn validate_outbound_url(
    raw: &str,
    allow_private: bool,
    require_https: bool,
    allowed_hosts: Option<&[&str]>,
) -> Result<Url> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("URL is required");
    }
    let parsed = Url::parse(trimmed).context("Invalid URL")?;
    let scheme = parsed.scheme();
    if require_https {
        if scheme != "https" {
            bail!("URL must use https://");
        }
    } else if scheme != "http" && scheme != "https" {
        bail!("URL must use http:// or https://");
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("URL must not embed username or password credentials");
    }

    let Some(host_str) = parsed.host_str() else {
        bail!("URL must include a valid host");
    };
    let host_lower = host_str.trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();

    if let Some(allowed) = allowed_hosts
        && !allowed.iter().any(|h| *h == host_lower)
    {
        bail!("URL host '{host_lower}' is not permitted for this integration");
    }

    // Always block cloud metadata hostnames
    if !allow_private
        && (matches!(
            host_lower.as_str(),
            "metadata.google.internal" | "metadata" | "instance-data"
        ) || host_lower.ends_with(".internal"))
    {
        bail!("Refusing internal/metadata hostname '{host_lower}'");
    }

    if !allow_private {
        if host_lower == "localhost"
            || host_lower.ends_with(".localhost")
            || host_lower.ends_with(".local")
        {
            bail!("Refusing local/loopback hostname '{host_lower}'");
        }

        if let Ok(ip) = host_lower.parse::<IpAddr>()
            && is_forbidden_ip(ip)
        {
            bail!("Refusing private, loopback, or link-local IP address '{ip}'");
        }
    }

    Ok(parsed)
}

/// Resolves DNS when needed (for SSRF checks at request time / redirect time).
pub async fn validate_outbound_url_with_dns(
    raw: &str,
    allow_private: bool,
    require_https: bool,
) -> Result<Url> {
    let parsed = validate_outbound_url(raw, allow_private, require_https, None)?;
    if allow_private {
        return Ok(parsed);
    }
    if let Some(host) = parsed.host_str() {
        let host_clean = host.trim_matches(|c| c == '[' || c == ']');
        if host_clean.parse::<IpAddr>().is_err() {
            let port = parsed.port_or_known_default().unwrap_or(443);
            if let Ok(addrs) = tokio::net::lookup_host((host_clean, port)).await {
                for addr in addrs {
                    if is_forbidden_ip(addr.ip()) {
                        bail!(
                            "Refusing host '{host_clean}' resolving to forbidden IP {}",
                            addr.ip()
                        );
                    }
                }
            }
        }
    }
    Ok(parsed)
}

pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_ipv4(v4),
        IpAddr::V6(v6) => is_forbidden_ipv6(v6),
    }
}

fn is_forbidden_ipv4(v4: Ipv4Addr) -> bool {
    let octets = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_multicast()
        || v4.is_broadcast()
        || octets[0] == 0
        // Carrier-grade NAT 100.64.0.0/10
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        // Benchmarking 198.18.0.0/15
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        // Documentation / reserved
        || octets[0] >= 240
}

fn is_forbidden_ipv6(v6: Ipv6Addr) -> bool {
    if let Some(mapped) = v6.to_ipv4_mapped() {
        return is_forbidden_ipv4(mapped);
    }
    let segs = v6.segments();
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        // Unique local addresses fc00::/7
        || (segs[0] & 0xfe00) == 0xfc00
        // Link-local fe80::/10
        || (segs[0] & 0xffc0) == 0xfe80
        // Documentation 2001:db8::/32
        || (segs[0] == 0x2001 && segs[1] == 0x0db8)
}

pub async fn validate_channel_config_urls(
    kind: crate::models::ChannelKind,
    config: &serde_json::Value,
    allow_private: bool,
) -> Result<()> {
    use crate::models::ChannelKind;
    match kind {
        ChannelKind::Discord => {
            let url = config
                .get("webhook_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            validate_outbound_url(
                url,
                allow_private,
                true,
                Some(&[
                    "discord.com",
                    "discordapp.com",
                    "canary.discord.com",
                    "ptb.discord.com",
                ]),
            )?;
        }
        ChannelKind::Gotify => {
            let url = config.get("url").and_then(|v| v.as_str()).unwrap_or("");
            validate_outbound_url_with_dns(url, allow_private, false).await?;
        }
        ChannelKind::Ntfy => {
            let url = config
                .get("url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("https://ntfy.sh");
            validate_outbound_url_with_dns(url, allow_private, false).await?;
        }
        ChannelKind::Telegram => {
            // Telegram uses hardcoded https://api.telegram.org in notify/telegram.rs
        }
    }
    Ok(())
}

/// Validates that a URL is strictly http/https (used to block `javascript:` / `data:` URLs).
pub fn is_safe_http_url(raw: &str) -> bool {
    let Ok(parsed) = Url::parse(raw.trim()) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cipher_roundtrip_and_legacy_plaintext() {
        let cipher = SecretCipher::ephemeral().unwrap();
        let enc = cipher.encrypt("uid=123; pass=secret").unwrap();
        assert!(enc.starts_with("enc:v1:"));
        assert_ne!(enc, "uid=123; pass=secret");
        assert_eq!(cipher.decrypt(&enc).unwrap(), "uid=123; pass=secret");
        // Legacy plaintext passes through transparently
        assert_eq!(cipher.decrypt("Cookie: a=b").unwrap(), "Cookie: a=b");
        assert_eq!(cipher.encrypt("").unwrap(), "");
    }

    #[test]
    fn argon2_password_hash_and_verify() {
        let hash = hash_password("correct-horse-battery-staple").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct-horse-battery-staple", &hash));
        assert!(!verify_password("wrong-password", &hash));
    }

    #[test]
    fn ssrf_blocks_metadata_loopback_and_private_ips() {
        assert!(validate_outbound_url("http://169.254.169.254/latest/meta-data/", false, false, None).is_err());
        assert!(validate_outbound_url("http://127.0.0.1:8085/api/overview", false, false, None).is_err());
        assert!(validate_outbound_url("http://[::1]:8085/", false, false, None).is_err());
        assert!(validate_outbound_url("http://10.0.0.5/", false, false, None).is_err());
        assert!(validate_outbound_url("http://192.168.1.1/", false, false, None).is_err());
        assert!(validate_outbound_url("http://localhost:8080/", false, false, None).is_err());
        assert!(validate_outbound_url("file:///etc/passwd", false, false, None).is_err());
        assert!(validate_outbound_url("https://user:pass@tracker.example/", false, false, None).is_err());
        assert!(validate_outbound_url("https://tracker.example/", false, false, None).is_ok());
        // With allow_private = true, local LAN works
        assert!(validate_outbound_url("http://192.168.1.50:8080", true, false, None).is_ok());
    }

    #[test]
    fn rate_limiter_blocks_after_threshold_and_lockout_activates() {
        let limiter = AuthRateLimiter::new();
        let ip: IpAddr = "203.0.113.10".parse().unwrap();
        for _ in 0..12 {
            assert!(limiter.check_ip_rate(ip, RateBucket::Auth).is_ok());
        }
        assert!(limiter.check_ip_rate(ip, RateBucket::Auth).is_err());

        for _ in 0..5 {
            limiter.record_login_failure("admin");
        }
        assert!(limiter.check_account_lockout("admin").is_err());
        limiter.record_login_success("admin");
        assert!(limiter.check_account_lockout("admin").is_ok());
    }
}
