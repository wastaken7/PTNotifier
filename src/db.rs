use crate::models::{
    ChannelConfig, ChannelKind, CreateChannelRequest, CreateTrackerRequest, GlobalSettings,
    NotificationHistoryEntry, NotificationItem, RssTokenPublic, SessionPublic, SessionRecord,
    TrackerConfig, TrackerState, TrackerStatus, UpdateChannelRequest, UpdateTrackerRequest,
    UserRecord, UserRole,
};
use crate::security::SecretCipher;
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    cipher: SecretCipher,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create database directory {}", parent.display())
            })?;
        }
        let cipher = SecretCipher::load_or_create_for_db(path)?;
        let conn = Connection::open(path).context("failed to open SQLite database")?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            cipher,
        };
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let cipher = SecretCipher::ephemeral()?;
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            cipher,
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                data_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS trackers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                base_url TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                check_interval_secs INTEGER,
                mark_as_read INTEGER,
                cookies_raw TEXT NOT NULL DEFAULT '',
                api_token TEXT NOT NULL DEFAULT '',
                ignore_patterns_json TEXT NOT NULL DEFAULT '[]',
                state_json TEXT NOT NULL DEFAULT '{}',
                last_status TEXT NOT NULL DEFAULT 'pending',
                last_error TEXT,
                last_checked_at TEXT,
                next_check_at TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS channels (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                config_json TEXT NOT NULL,
                notify_notifications INTEGER NOT NULL DEFAULT 1,
                notify_pms INTEGER NOT NULL DEFAULT 1,
                notify_errors INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS notification_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tracker_id TEXT NOT NULL,
                tracker_name TEXT NOT NULL,
                item_type TEXT NOT NULL,
                external_id TEXT NOT NULL,
                title TEXT,
                subject TEXT,
                sender TEXT,
                body TEXT,
                is_staff INTEGER NOT NULL DEFAULT 0,
                item_date TEXT NOT NULL,
                url TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_history_created_at ON notification_history(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_history_tracker_id ON notification_history(tracker_id);

            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'admin',
                token_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_login_at TEXT
            );

            CREATE TABLE IF NOT EXISTS sessions (
                token_hash TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                token_version INTEGER NOT NULL,
                csrf_token TEXT NOT NULL,
                user_agent TEXT NOT NULL DEFAULT '',
                ip_address TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions(expires_at);

            CREATE TABLE IF NOT EXISTS rss_tokens (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                label TEXT NOT NULL,
                token_hash TEXT NOT NULL UNIQUE,
                token_prefix TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_used_at TEXT
            );",
        )?;

        let existing: Option<String> = conn
            .query_row("SELECT data_json FROM settings WHERE id = 1", [], |r| r.get(0))
            .optional()?;
        if existing.is_none() {
            let defaults = serde_json::to_string(&GlobalSettings::default())?;
            conn.execute(
                "INSERT INTO settings (id, data_json) VALUES (1, ?1)",
                params![defaults],
            )?;
        }

        // Encrypt any legacy plaintext credentials already stored in SQLite.
        let mut tracker_updates: Vec<(String, String, String)> = Vec::new();
        {
            let mut stmt = conn.prepare("SELECT id, cookies_raw, api_token FROM trackers")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for r in rows {
                let (id, cookies, token) = r?;
                if !SecretCipher::is_encrypted(&cookies) || !SecretCipher::is_encrypted(&token) {
                    let enc_cookies = if SecretCipher::is_encrypted(&cookies) {
                        cookies
                    } else {
                        self.cipher.encrypt(&cookies)?
                    };
                    let enc_token = if SecretCipher::is_encrypted(&token) {
                        token
                    } else {
                        self.cipher.encrypt(&token)?
                    };
                    tracker_updates.push((id, enc_cookies, enc_token));
                }
            }
        }
        for (id, enc_cookies, enc_token) in tracker_updates {
            conn.execute(
                "UPDATE trackers SET cookies_raw = ?1, api_token = ?2 WHERE id = ?3",
                params![enc_cookies, enc_token, id],
            )?;
        }

        let mut channel_updates: Vec<(String, String)> = Vec::new();
        {
            let mut stmt = conn.prepare("SELECT id, config_json FROM channels")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for r in rows {
                let (id, cfg) = r?;
                if !SecretCipher::is_encrypted(&cfg) {
                    channel_updates.push((id, self.cipher.encrypt(&cfg)?));
                }
            }
        }
        for (id, enc_cfg) in channel_updates {
            conn.execute(
                "UPDATE channels SET config_json = ?1 WHERE id = ?2",
                params![enc_cfg, id],
            )?;
        }

        Ok(())
    }

    pub fn get_settings(&self) -> Result<GlobalSettings> {
        let conn = self.conn.lock().unwrap();
        let json: String = conn.query_row("SELECT data_json FROM settings WHERE id = 1", [], |r| r.get(0))?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn update_settings(&self, settings: &GlobalSettings) -> Result<GlobalSettings> {
        let conn = self.conn.lock().unwrap();
        let json = serde_json::to_string(settings)?;
        conn.execute(
            "UPDATE settings SET data_json = ?1 WHERE id = 1",
            params![json],
        )?;
        Ok(settings.clone())
    }

    fn row_to_tracker(&self, row: &rusqlite::Row<'_>) -> Result<TrackerConfig> {
        let mark_as_read_int: Option<i64> = row.get(6)?;
        let cookies_stored: String = row.get(7)?;
        let token_stored: String = row.get(8)?;
        let ignore_json: String = row.get(9)?;
        let state_json: String = row.get(10)?;
        let status_str: String = row.get(11)?;

        Ok(TrackerConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            base_url: row.get(3)?,
            enabled: row.get::<_, i64>(4)? != 0,
            check_interval_secs: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
            mark_as_read: mark_as_read_int.map(|v| v != 0),
            cookies_raw: self.cipher.decrypt(&cookies_stored)?,
            api_token: self.cipher.decrypt(&token_stored)?,
            ignore_patterns: serde_json::from_str(&ignore_json).unwrap_or_default(),
            state: serde_json::from_str(&state_json).unwrap_or_default(),
            last_status: TrackerStatus::from_str(&status_str),
            last_error: row.get(12)?,
            last_checked_at: row.get(13)?,
            next_check_at: row.get(14)?,
            created_at: row.get(15)?,
        })
    }

    pub fn list_trackers(&self) -> Result<Vec<TrackerConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, base_url, enabled, check_interval_secs, mark_as_read,
                    cookies_raw, api_token, ignore_patterns_json, state_json, last_status,
                    last_error, last_checked_at, next_check_at, created_at
             FROM trackers ORDER BY created_at ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(self.row_to_tracker(r)?);
        }
        Ok(out)
    }

    pub fn get_tracker(&self, id: &str) -> Result<Option<TrackerConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, base_url, enabled, check_interval_secs, mark_as_read,
                    cookies_raw, api_token, ignore_patterns_json, state_json, last_status,
                    last_error, last_checked_at, next_check_at, created_at
             FROM trackers WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(self.row_to_tracker(r)?))
        } else {
            Ok(None)
        }
    }

    pub fn create_tracker(&self, req: CreateTrackerRequest, resolved_base_url: String) -> Result<TrackerConfig> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let status = if req.enabled {
            TrackerStatus::Pending
        } else {
            TrackerStatus::Paused
        };
        let ignore_json = serde_json::to_string(&req.ignore_patterns)?;
        let state_json = serde_json::to_string(&TrackerState::default())?;
        let enc_cookies = self.cipher.encrypt(&req.cookies_raw)?;
        let enc_token = self.cipher.encrypt(req.api_token.trim())?;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO trackers (
                id, name, kind, base_url, enabled, check_interval_secs, mark_as_read,
                cookies_raw, api_token, ignore_patterns_json, state_json, last_status, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                id,
                req.name.trim(),
                req.kind.trim(),
                resolved_base_url,
                if req.enabled { 1 } else { 0 },
                req.check_interval_secs.map(|v| v as i64),
                req.mark_as_read.map(|b| if b { 1 } else { 0 }),
                enc_cookies,
                enc_token,
                ignore_json,
                state_json,
                status.as_str(),
                now,
            ],
        )?;
        drop(conn);
        self.get_tracker(&id)?
            .ok_or_else(|| anyhow!("failed to read back created tracker"))
    }

    pub fn update_tracker(&self, id: &str, req: UpdateTrackerRequest) -> Result<Option<TrackerConfig>> {
        let Some(mut current) = self.get_tracker(id)? else {
            return Ok(None);
        };

        if let Some(name) = req.name {
            current.name = name.trim().to_string();
        }
        if let Some(kind) = req.kind {
            current.kind = kind.trim().to_string();
        }
        if let Some(base_url) = req.base_url {
            current.base_url = base_url.trim().to_string();
        }
        if let Some(enabled) = req.enabled {
            current.enabled = enabled;
            if !enabled {
                current.last_status = TrackerStatus::Paused;
            } else if current.last_status == TrackerStatus::Paused {
                current.last_status = TrackerStatus::Pending;
            }
        }
        if let Some(interval) = req.check_interval_secs {
            current.check_interval_secs = interval;
        }
        if let Some(mar) = req.mark_as_read {
            current.mark_as_read = mar;
        }
        if req.clear_cookies {
            current.cookies_raw.clear();
        } else if let Some(cookies) = req.cookies_raw
            && !cookies.trim().is_empty()
        {
            current.cookies_raw = cookies;
        }
        if req.clear_api_token {
            current.api_token.clear();
        } else if let Some(token) = req.api_token
            && !token.trim().is_empty()
        {
            current.api_token = token.trim().to_string();
        }
        if let Some(patterns) = req.ignore_patterns {
            current.ignore_patterns = patterns;
        }

        let ignore_json = serde_json::to_string(&current.ignore_patterns)?;
        let enc_cookies = self.cipher.encrypt(&current.cookies_raw)?;
        let enc_token = self.cipher.encrypt(&current.api_token)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE trackers SET
                name = ?1, kind = ?2, base_url = ?3, enabled = ?4,
                check_interval_secs = ?5, mark_as_read = ?6, cookies_raw = ?7,
                api_token = ?8, ignore_patterns_json = ?9, last_status = ?10
             WHERE id = ?11",
            params![
                current.name,
                current.kind,
                current.base_url,
                if current.enabled { 1 } else { 0 },
                current.check_interval_secs.map(|v| v as i64),
                current.mark_as_read.map(|b| if b { 1 } else { 0 }),
                enc_cookies,
                enc_token,
                ignore_json,
                current.last_status.as_str(),
                id,
            ],
        )?;
        drop(conn);
        self.get_tracker(id)
    }

    pub fn update_tracker_runtime(
        &self,
        id: &str,
        status: TrackerStatus,
        last_error: Option<&str>,
        last_checked_at: Option<&str>,
        next_check_at: Option<&str>,
        state: &TrackerState,
    ) -> Result<Option<TrackerConfig>> {
        let state_json = serde_json::to_string(state)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE trackers SET
                last_status = ?1,
                last_error = ?2,
                last_checked_at = COALESCE(?3, last_checked_at),
                next_check_at = ?4,
                state_json = ?5
             WHERE id = ?6",
            params![
                status.as_str(),
                last_error,
                last_checked_at,
                next_check_at,
                state_json,
                id,
            ],
        )?;
        drop(conn);
        self.get_tracker(id)
    }

    pub fn delete_tracker(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM trackers WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    fn row_to_channel(&self, row: &rusqlite::Row<'_>) -> Result<ChannelConfig> {
        let kind_str: String = row.get(2)?;
        let config_stored: String = row.get(4)?;
        let config_json = self.cipher.decrypt(&config_stored)?;
        Ok(ChannelConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: ChannelKind::from_str(&kind_str).unwrap_or(ChannelKind::Discord),
            enabled: row.get::<_, i64>(3)? != 0,
            config: serde_json::from_str(&config_json).unwrap_or(serde_json::json!({})),
            notify_notifications: row.get::<_, i64>(5)? != 0,
            notify_pms: row.get::<_, i64>(6)? != 0,
            notify_errors: row.get::<_, i64>(7)? != 0,
            created_at: row.get(8)?,
        })
    }

    pub fn list_channels(&self) -> Result<Vec<ChannelConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, enabled, config_json,
                    notify_notifications, notify_pms, notify_errors, created_at
             FROM channels ORDER BY created_at ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(self.row_to_channel(r)?);
        }
        Ok(out)
    }

    pub fn get_channel(&self, id: &str) -> Result<Option<ChannelConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, enabled, config_json,
                    notify_notifications, notify_pms, notify_errors, created_at
             FROM channels WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(self.row_to_channel(r)?))
        } else {
            Ok(None)
        }
    }

    pub fn create_channel(&self, req: CreateChannelRequest) -> Result<ChannelConfig> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let cfg_json = serde_json::to_string(&req.config)?;
        let enc_cfg = self.cipher.encrypt(&cfg_json)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO channels (
                id, name, kind, enabled, config_json,
                notify_notifications, notify_pms, notify_errors, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                req.name.trim(),
                req.kind.as_str(),
                if req.enabled { 1 } else { 0 },
                enc_cfg,
                if req.notify_notifications { 1 } else { 0 },
                if req.notify_pms { 1 } else { 0 },
                if req.notify_errors { 1 } else { 0 },
                now,
            ],
        )?;
        drop(conn);
        self.get_channel(&id)?
            .ok_or_else(|| anyhow!("failed to read back created channel"))
    }

    pub fn update_channel(&self, id: &str, req: UpdateChannelRequest) -> Result<Option<ChannelConfig>> {
        let Some(mut current) = self.get_channel(id)? else {
            return Ok(None);
        };
        if let Some(name) = req.name {
            current.name = name.trim().to_string();
        }
        if let Some(enabled) = req.enabled {
            current.enabled = enabled;
        }
        if let Some(mut cfg) = req.config {
            if let (Some(new_obj), Some(old_obj)) = (cfg.as_object_mut(), current.config.as_object()) {
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
            current.config = cfg;
        }
        if let Some(v) = req.notify_notifications {
            current.notify_notifications = v;
        }
        if let Some(v) = req.notify_pms {
            current.notify_pms = v;
        }
        if let Some(v) = req.notify_errors {
            current.notify_errors = v;
        }

        let cfg_json = serde_json::to_string(&current.config)?;
        let enc_cfg = self.cipher.encrypt(&cfg_json)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE channels SET
                name = ?1, enabled = ?2, config_json = ?3,
                notify_notifications = ?4, notify_pms = ?5, notify_errors = ?6
             WHERE id = ?7",
            params![
                current.name,
                if current.enabled { 1 } else { 0 },
                enc_cfg,
                if current.notify_notifications { 1 } else { 0 },
                if current.notify_pms { 1 } else { 0 },
                if current.notify_errors { 1 } else { 0 },
                id,
            ],
        )?;
        drop(conn);
        self.get_channel(id)
    }

    pub fn delete_channel(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM channels WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    pub fn insert_notification_history(
        &self,
        tracker_id: &str,
        tracker_name: &str,
        item: &NotificationItem,
    ) -> Result<NotificationHistoryEntry> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO notification_history (
                tracker_id, tracker_name, item_type, external_id,
                title, subject, sender, body, is_staff, item_date, url, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                tracker_id,
                tracker_name,
                item.item_type.as_str(),
                item.id,
                item.title,
                item.subject,
                item.sender,
                item.body,
                if item.is_staff { 1 } else { 0 },
                item.date,
                item.url,
                now,
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(NotificationHistoryEntry {
            id,
            tracker_id: tracker_id.to_string(),
            tracker_name: tracker_name.to_string(),
            item_type: item.item_type.as_str().to_string(),
            external_id: item.id.clone(),
            title: item.title.clone(),
            subject: item.subject.clone(),
            sender: item.sender.clone(),
            body: item.body.clone(),
            is_staff: item.is_staff,
            item_date: item.date.clone(),
            url: item.url.clone(),
            created_at: now,
        })
    }

    pub fn list_notification_history(
        &self,
        tracker_id: Option<&str>,
        item_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<NotificationHistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, tracker_id, tracker_name, item_type, external_id,
                    title, subject, sender, body, is_staff, item_date, url, created_at
             FROM notification_history WHERE 1=1",
        );
        let mut conditions: Vec<String> = Vec::new();
        if let Some(tid) = tracker_id {
            sql.push_str(&format!(" AND tracker_id = ?{}", conditions.len() + 1));
            conditions.push(tid.to_string());
        }
        if let Some(it) = item_type {
            sql.push_str(&format!(" AND item_type = ?{}", conditions.len() + 1));
            conditions.push(it.to_string());
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT ?{}", conditions.len() + 1));

        let mut stmt = conn.prepare(&sql)?;
        let limit_i64 = limit.min(500) as i64;
        let rows = match conditions.len() {
            0 => stmt.query_map(params![limit_i64], Self::row_to_history)?,
            1 => stmt.query_map(params![conditions[0], limit_i64], Self::row_to_history)?,
            _ => stmt.query_map(
                params![conditions[0], conditions[1], limit_i64],
                Self::row_to_history,
            )?,
        };
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn row_to_history(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotificationHistoryEntry> {
        Ok(NotificationHistoryEntry {
            id: row.get(0)?,
            tracker_id: row.get(1)?,
            tracker_name: row.get(2)?,
            item_type: row.get(3)?,
            external_id: row.get(4)?,
            title: row.get(5)?,
            subject: row.get(6)?,
            sender: row.get(7)?,
            body: row.get(8)?,
            is_staff: row.get::<_, i64>(9)? != 0,
            item_date: row.get(10)?,
            url: row.get(11)?,
            created_at: row.get(12)?,
        })
    }

    fn row_to_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRecord> {
        let role_str: String = row.get(3)?;
        Ok(UserRecord {
            id: row.get(0)?,
            username: row.get(1)?,
            password_hash: row.get(2)?,
            role: UserRole::from_str(&role_str),
            token_version: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            last_login_at: row.get(7)?,
        })
    }

    pub fn count_users(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(count.max(0) as usize)
    }

    pub fn create_user(&self, username: &str, password_hash: &str, role: UserRole) -> Result<UserRecord> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let exists: Option<String> = conn
            .query_row(
                "SELECT id FROM users WHERE LOWER(username) = LOWER(?1)",
                params![username],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_some() {
            bail!("Username already exists");
        }
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, token_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![id, username, password_hash, role.as_str(), now, now],
        )?;
        drop(conn);
        self.get_user_by_id(&id)?
            .ok_or_else(|| anyhow!("failed to read back created user"))
    }

    /// Creates the first administrator only if no user exists yet. The emptiness check and the
    /// insert run in one immediate transaction under the connection lock, so concurrent setup
    /// requests cannot both succeed. Returns `None` when setup was already completed.
    pub fn create_initial_admin(&self, username: &str, password_hash: &str) -> Result<Option<UserRecord>> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
            if count > 0 {
                return Ok(None);
            }
            tx.execute(
                "INSERT INTO users (id, username, password_hash, role, token_version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
                params![id, username, password_hash, UserRole::Admin.as_str(), now, now],
            )?;
            tx.commit()?;
        }
        self.get_user_by_id(&id)
    }

    pub fn get_user_by_id(&self, id: &str) -> Result<Option<UserRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, username, password_hash, role, token_version, created_at, updated_at, last_login_at
             FROM users WHERE id = ?1",
            params![id],
            Self::row_to_user,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<UserRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, username, password_hash, role, token_version, created_at, updated_at, last_login_at
             FROM users WHERE LOWER(username) = LOWER(?1)",
            params![username.trim()],
            Self::row_to_user,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_users(&self) -> Result<Vec<UserRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, password_hash, role, token_version, created_at, updated_at, last_login_at
             FROM users ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], Self::row_to_user)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn update_user_password(
        &self,
        user_id: &str,
        new_password_hash: &str,
        keep_session_hash: Option<&str>,
    ) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE users SET password_hash = ?1, token_version = token_version + 1, updated_at = ?2
             WHERE id = ?3",
            params![new_password_hash, now, user_id],
        )?;
        if affected == 0 {
            return Ok(false);
        }
        if let Some(keep) = keep_session_hash {
            let new_ver: i64 = conn.query_row(
                "SELECT token_version FROM users WHERE id = ?1",
                params![user_id],
                |r| r.get(0),
            )?;
            conn.execute(
                "UPDATE sessions SET token_version = ?1 WHERE user_id = ?2 AND token_hash = ?3",
                params![new_ver, user_id, keep],
            )?;
            conn.execute(
                "DELETE FROM sessions WHERE user_id = ?1 AND token_hash != ?2",
                params![user_id, keep],
            )?;
        } else {
            conn.execute("DELETE FROM sessions WHERE user_id = ?1", params![user_id])?;
        }
        Ok(true)
    }

    pub fn update_user_last_login(&self, user_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET last_login_at = ?1 WHERE id = ?2",
            params![now, user_id],
        )?;
        Ok(())
    }

    pub fn delete_user(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let target_role: Option<String> = conn
            .query_row("SELECT role FROM users WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?;
        let Some(role) = target_role else {
            return Ok(false);
        };
        if role == "admin" {
            let admin_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM users WHERE role = 'admin'",
                [],
                |r| r.get(0),
            )?;
            if admin_count <= 1 {
                bail!("Cannot delete the last administrator account");
            }
        }
        let affected = conn.execute("DELETE FROM users WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_session(
        &self,
        token_hash: &str,
        user_id: &str,
        token_version: i64,
        csrf_token: &str,
        user_agent: &str,
        ip_address: &str,
        expires_at: &str,
    ) -> Result<SessionRecord> {
        let now = Utc::now().to_rfc3339();
        let ua: String = user_agent.chars().take(240).collect();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (
                token_hash, user_id, token_version, csrf_token, user_agent, ip_address,
                created_at, expires_at, last_seen_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                token_hash,
                user_id,
                token_version,
                csrf_token,
                ua,
                ip_address,
                now,
                expires_at,
                now,
            ],
        )?;
        Ok(SessionRecord {
            token_hash: token_hash.to_string(),
            token_version,
            csrf_token: csrf_token.to_string(),
            expires_at: expires_at.to_string(),
            last_seen_at: now,
        })
    }

    pub fn get_valid_session(&self, token_hash: &str) -> Result<Option<(SessionRecord, UserRecord)>> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT s.token_hash, s.token_version, s.csrf_token,
                        s.expires_at, s.last_seen_at,
                        u.id, u.username, u.password_hash, u.role, u.token_version,
                        u.created_at, u.updated_at, u.last_login_at
                 FROM sessions s
                 JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1",
                params![token_hash],
                |r| {
                    let sess = SessionRecord {
                        token_hash: r.get(0)?,
                        token_version: r.get(1)?,
                        csrf_token: r.get(2)?,
                        expires_at: r.get(3)?,
                        last_seen_at: r.get(4)?,
                    };
                    let role_str: String = r.get(8)?;
                    let user = UserRecord {
                        id: r.get(5)?,
                        username: r.get(6)?,
                        password_hash: r.get(7)?,
                        role: UserRole::from_str(&role_str),
                        token_version: r.get(9)?,
                        created_at: r.get(10)?,
                        updated_at: r.get(11)?,
                        last_login_at: r.get(12)?,
                    };
                    Ok((sess, user))
                },
            )
            .optional()?;

        let Some((mut sess, user)) = row else {
            return Ok(None);
        };

        if sess.expires_at <= now || sess.token_version != user.token_version {
            let _ = conn.execute("DELETE FROM sessions WHERE token_hash = ?1", params![token_hash]);
            return Ok(None);
        }

        let _ = conn.execute(
            "UPDATE sessions SET last_seen_at = ?1 WHERE token_hash = ?2",
            params![now, token_hash],
        );
        sess.last_seen_at = now;
        Ok(Some((sess, user)))
    }

    /// Read-only validity check (does not refresh `last_seen_at`), for long-lived connections
    /// that must stop once their session expires or is revoked.
    pub fn is_session_active(&self, token_hash: &str) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let active = conn
            .query_row(
                "SELECT 1 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND s.token_version = u.token_version",
                params![token_hash, now],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(active)
    }

    pub fn list_user_sessions(
        &self,
        user_id: &str,
        current_token_hash: &str,
    ) -> Result<Vec<SessionPublic>> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now]);
        let mut stmt = conn.prepare(
            "SELECT token_hash, user_agent, ip_address, created_at, expires_at, last_seen_at
             FROM sessions WHERE user_id = ?1 ORDER BY last_seen_at DESC",
        )?;
        let rows = stmt.query_map(params![user_id], |r| {
            let hash: String = r.get(0)?;
            Ok(SessionPublic {
                id_prefix: hash.chars().take(12).collect(),
                current: hash == current_token_hash,
                user_agent: r.get(1)?,
                ip_address: r.get(2)?,
                created_at: r.get(3)?,
                expires_at: r.get(4)?,
                last_seen_at: r.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_session(&self, token_hash: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM sessions WHERE token_hash = ?1", params![token_hash])?;
        Ok(affected > 0)
    }

    pub fn delete_session_by_prefix(&self, user_id: &str, prefix: &str) -> Result<bool> {
        if prefix.len() < 8 {
            return Ok(false);
        }
        let conn = self.conn.lock().unwrap();
        let pattern = format!("{prefix}%");
        let affected = conn.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND token_hash LIKE ?2",
            params![user_id, pattern],
        )?;
        Ok(affected > 0)
    }

    pub fn delete_other_sessions(&self, user_id: &str, keep_token_hash: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND token_hash != ?2",
            params![user_id, keep_token_hash],
        )?;
        Ok(affected)
    }

    pub fn prune_expired_sessions(&self) -> Result<usize> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
        Ok(affected)
    }

    pub fn create_rss_token(
        &self,
        user_id: &str,
        label: &str,
        token_hash: &str,
        token_prefix: &str,
    ) -> Result<RssTokenPublic> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let clean_label = if label.trim().is_empty() {
            "RSS Reader".to_string()
        } else {
            label.trim().chars().take(64).collect()
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO rss_tokens (id, user_id, label, token_hash, token_prefix, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, user_id, clean_label, token_hash, token_prefix, now],
        )?;
        Ok(RssTokenPublic {
            id,
            label: clean_label,
            token_prefix: token_prefix.to_string(),
            created_at: now,
            last_used_at: None,
        })
    }

    pub fn list_rss_tokens(&self, user_id: &str) -> Result<Vec<RssTokenPublic>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, label, token_prefix, created_at, last_used_at
             FROM rss_tokens WHERE user_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![user_id], |r| {
            Ok(RssTokenPublic {
                id: r.get(0)?,
                label: r.get(1)?,
                token_prefix: r.get(2)?,
                created_at: r.get(3)?,
                last_used_at: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn verify_rss_token(&self, token_hash: &str) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE rss_tokens SET last_used_at = ?1 WHERE token_hash = ?2",
            params![now, token_hash],
        )?;
        Ok(affected > 0)
    }

    pub fn delete_rss_token(&self, user_id: &str, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "DELETE FROM rss_tokens WHERE id = ?1 AND user_id = ?2",
            params![id, user_id],
        )?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ItemType;

    #[test]
    fn crud_trackers_channels_and_history() {
        let db = Database::open_in_memory().unwrap();
        let settings = db.get_settings().unwrap();
        assert_eq!(settings.default_check_interval_secs, 1800);

        let tracker = db
            .create_tracker(
                CreateTrackerRequest {
                    name: "MyUNIT3D".into(),
                    kind: "unit3d".into(),
                    base_url: None,
                    enabled: true,
                    check_interval_secs: Some(2400),
                    mark_as_read: Some(true),
                    cookies_raw: "Cookie: a=b".into(),
                    api_token: String::new(),
                    ignore_patterns: vec!["deleted".into()],
                },
                "https://example.org".into(),
            )
            .unwrap();
        assert_eq!(tracker.name, "MyUNIT3D");
        assert_eq!(db.list_trackers().unwrap().len(), 1);

        let ch = db
            .create_channel(CreateChannelRequest {
                name: "Main Discord".into(),
                kind: ChannelKind::Discord,
                enabled: true,
                config: serde_json::json!({"webhook_url": "https://discord.com/api/webhooks/1/a"}),
                notify_notifications: true,
                notify_pms: true,
                notify_errors: false,
            })
            .unwrap();
        assert_eq!(ch.kind, ChannelKind::Discord);
        assert_eq!(db.list_channels().unwrap().len(), 1);

        let item = NotificationItem {
            item_type: ItemType::Message,
            id: "123".into(),
            title: Some("Title".into()),
            subject: Some("Hello".into()),
            sender: Some("Admin".into()),
            body: Some("Body text".into()),
            date: "2026-11-13".into(),
            url: "https://example.org/msg/123".into(),
            is_staff: true,
            favicon: None,
            mark_read_url: None,
        };
        db.insert_notification_history(&tracker.id, &tracker.name, &item)
            .unwrap();
        let hist = db.list_notification_history(None, None, 10).unwrap();
        assert_eq!(hist.len(), 1);
        assert!(hist[0].is_staff);
    }

    #[test]
    fn open_creates_missing_parent_directories() {
        let base = std::env::temp_dir().join(format!("ptn_test_{}", Uuid::new_v4()));
        let db_path = base.join("nested").join("ptnotifier.db");
        let db = Database::open(&db_path).unwrap();
        assert!(db_path.exists());
        drop(db);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn secrets_are_encrypted_at_rest_and_sessions_follow_token_version() {
        let db = Database::open_in_memory().unwrap();
        let tracker = db
            .create_tracker(
                CreateTrackerRequest {
                    name: "SecretTracker".into(),
                    kind: "unit3d".into(),
                    base_url: None,
                    enabled: true,
                    check_interval_secs: None,
                    mark_as_read: None,
                    cookies_raw: "Cookie: pass=supersecret123".into(),
                    api_token: "tok_secret_xyz".into(),
                    ignore_patterns: vec![],
                },
                "https://tracker.example".into(),
            )
            .unwrap();

        assert_eq!(tracker.cookies_raw, "Cookie: pass=supersecret123");
        assert_eq!(tracker.api_token, "tok_secret_xyz");

        {
            let conn = db.conn.lock().unwrap();
            let (raw_cookies, raw_token): (String, String) = conn
                .query_row(
                    "SELECT cookies_raw, api_token FROM trackers WHERE id = ?1",
                    params![tracker.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert!(raw_cookies.starts_with("enc:v1:"));
            assert!(raw_token.starts_with("enc:v1:"));
            assert!(!raw_cookies.contains("supersecret123"));
            assert!(!raw_token.contains("tok_secret_xyz"));
        }

        let user = db
            .create_user("admin", "$argon2id$dummy", UserRole::Admin)
            .unwrap();
        assert!(db.delete_user(&user.id).is_err());

        let exp = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        db.create_session("hash1", &user.id, 1, "csrf1", "Mozilla", "1.2.3.4", &exp)
            .unwrap();
        db.create_session("hash2", &user.id, 1, "csrf2", "Mozilla", "1.2.3.4", &exp)
            .unwrap();
        assert!(db.get_valid_session("hash1").unwrap().is_some());

        db.update_user_password(&user.id, "$argon2id$new", Some("hash1"))
            .unwrap();
        assert!(db.get_valid_session("hash1").unwrap().is_some());
        assert!(db.get_valid_session("hash2").unwrap().is_none());

        let rss = db
            .create_rss_token(&user.id, "Feedly", "rsshash1", "ptnrss_abc")
            .unwrap();
        assert!(db.verify_rss_token("rsshash1").unwrap());
        assert!(db.delete_rss_token(&user.id, &rss.id).unwrap());
        assert!(!db.verify_rss_token("rsshash1").unwrap());
    }

    #[test]
    fn initial_admin_is_created_only_once_and_session_activity_tracks_revocation() {
        let db = Database::open_in_memory().unwrap();
        let first = db.create_initial_admin("admin", "$argon2id$a").unwrap();
        let user = first.expect("first setup creates the admin");
        assert_eq!(user.role, UserRole::Admin);
        assert!(db.create_initial_admin("other", "$argon2id$b").unwrap().is_none());
        assert_eq!(db.count_users().unwrap(), 1);

        let exp = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        db.create_session("live", &user.id, 1, "csrf", "UA", "1.2.3.4", &exp)
            .unwrap();
        assert!(db.is_session_active("live").unwrap());
        assert!(!db.is_session_active("missing").unwrap());

        db.update_user_password(&user.id, "$argon2id$new", None).unwrap();
        assert!(!db.is_session_active("live").unwrap());
    }
}
