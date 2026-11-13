use crate::db::Database;
use crate::models::{ItemType, NotificationItem, TrackerConfig, TrackerStatus, UiEvent};
use crate::notify;
use crate::trackers::{self, TrackerContext, TrackerError, find_preset};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use regex::Regex;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{error, info, warn};

#[derive(Debug)]
pub enum SchedulerCmd {
    Reload,
    RunNow(String),
}

#[derive(Clone)]
pub struct SchedulerHandle {
    tx: mpsc::Sender<SchedulerCmd>,
}

impl SchedulerHandle {
    #[cfg(test)]
    pub fn new_for_test(tx: mpsc::Sender<SchedulerCmd>) -> Self {
        Self { tx }
    }

    pub async fn reload(&self) {
        let _ = self.tx.send(SchedulerCmd::Reload).await;
    }

    pub async fn run_now(&self, tracker_id: String) {
        let _ = self.tx.send(SchedulerCmd::RunNow(tracker_id)).await;
    }
}

pub fn effective_interval_secs(tracker: &TrackerConfig, default_secs: u64) -> u64 {
    let min_secs = find_preset(&tracker.kind)
        .map(|p| p.min_interval_secs)
        .unwrap_or(1800);
    tracker
        .check_interval_secs
        .unwrap_or(default_secs)
        .max(min_secs)
}

fn should_ignore(item: &NotificationItem, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let haystack = format!(
        "{}\n{}\n{}\n{}",
        item.title.as_deref().unwrap_or(""),
        item.subject.as_deref().unwrap_or(""),
        item.sender.as_deref().unwrap_or(""),
        item.body.as_deref().unwrap_or("")
    );
    let lower = haystack.to_lowercase();

    for pat in patterns {
        let p = pat.trim();
        if p.is_empty() {
            continue;
        }
        if p.starts_with('/') && p.ends_with('/') && p.len() > 2 {
            let inner = &p[1..p.len() - 1];
            if let Ok(re) = Regex::new(&format!("(?i){inner}"))
                && re.is_match(&haystack)
            {
                return true;
            }
        } else if lower.contains(&p.to_lowercase()) {
            return true;
        }
    }
    false
}

pub async fn execute_tracker_check(
    db: &Database,
    rate_limiter: Arc<Mutex<Instant>>,
    event_tx: &broadcast::Sender<UiEvent>,
    notify_client: &Client,
    tracker_id: &str,
) -> anyhow::Result<()> {
    let Some(tracker) = db.get_tracker(tracker_id)? else {
        return Ok(());
    };
    let settings = db.get_settings()?;
    let interval_secs = effective_interval_secs(&tracker, settings.default_check_interval_secs);

    if let Ok(Some(updated)) = db.update_tracker_runtime(
        tracker_id,
        TrackerStatus::Running,
        None,
        None,
        tracker.next_check_at.as_deref(),
        &tracker.state,
    ) {
        let _ = event_tx.send(UiEvent::TrackerUpdated(updated.to_public()));
    }

    info!(tracker = %tracker.name, kind = %tracker.kind, "Checking tracker...");

    let mut ctx = match TrackerContext::new(tracker.clone(), settings.clone(), rate_limiter) {
        Ok(c) => c,
        Err(e) => {
            let now = Utc::now();
            let next = (now + ChronoDuration::seconds(interval_secs as i64)).to_rfc3339();
            if let Ok(Some(updated)) = db.update_tracker_runtime(
                tracker_id,
                TrackerStatus::Error,
                Some(&e.to_string()),
                Some(&now.to_rfc3339()),
                Some(&next),
                &tracker.state,
            ) {
                let _ = event_tx.send(UiEvent::TrackerUpdated(updated.to_public()));
            }
            return Err(e.into());
        }
    };

    let result = trackers::fetch_items(&mut ctx).await;
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let next_str = (now + ChronoDuration::seconds(interval_secs as i64)).to_rfc3339();

    match result {
        Ok(items) => {
            if !ctx.state.initialized {
                let count = items.len();
                for item in &items {
                    ctx.mark_processed(&item.id);
                }
                ctx.state.initialized = true;
                info!(
                    tracker = %tracker.name,
                    seeded = count,
                    "First check complete: seeded existing items without notifying"
                );
            } else {
                let channels = db.list_channels().unwrap_or_default();
                let mut sent = 0usize;
                for item in &items {
                    if ctx.is_processed(&item.id) {
                        continue;
                    }
                    ctx.mark_processed(&item.id);

                    if should_ignore(item, &tracker.ignore_patterns) {
                        info!(tracker = %tracker.name, id = %item.id, "Ignored item matching filter");
                        continue;
                    }

                    if let Ok(entry) =
                        db.insert_notification_history(&tracker.id, &tracker.name, item)
                    {
                        let _ = event_tx.send(UiEvent::NewNotification(entry));
                    }

                    notify::dispatch_item(
                        notify_client,
                        &channels,
                        &settings,
                        &tracker.name,
                        &tracker.base_url,
                        item,
                    )
                    .await;

                    trackers::mark_item_read(&ctx, item).await;
                    sent += 1;
                }
                if sent > 0 {
                    info!(tracker = %tracker.name, count = sent, "Dispatched new notification(s)");
                } else {
                    info!(tracker = %tracker.name, "No new notifications");
                }
            }

            if let Ok(Some(updated)) = db.update_tracker_runtime(
                tracker_id,
                TrackerStatus::Ok,
                None,
                Some(&now_str),
                Some(&next_str),
                &ctx.state,
            ) {
                let _ = event_tx.send(UiEvent::TrackerUpdated(updated.to_public()));
            }
        }
        Err(err) => {
            let status = match &err {
                TrackerError::CookieExpired(_) => TrackerStatus::CookieExpired,
                _ => TrackerStatus::Error,
            };
            let msg = err.to_string();
            error!(tracker = %tracker.name, error = %msg, "Tracker check failed");

            if settings.send_error_notifications {
                let channels = db.list_channels().unwrap_or_default();
                let err_item = NotificationItem {
                    item_type: ItemType::Error,
                    id: format!("err-{}", now.timestamp()),
                    title: Some(format!("{} Check Failed", tracker.name)),
                    subject: Some(status.as_str().to_string()),
                    sender: Some("PTNotifier".into()),
                    body: Some(msg.clone()),
                    date: now_str.clone(),
                    url: tracker.base_url.clone(),
                    is_staff: false,
                    favicon: None,
                    mark_read_url: None,
                };
                if let Ok(entry) =
                    db.insert_notification_history(&tracker.id, &tracker.name, &err_item)
                {
                    let _ = event_tx.send(UiEvent::NewNotification(entry));
                }
                notify::dispatch_item(
                    notify_client,
                    &channels,
                    &settings,
                    &tracker.name,
                    &tracker.base_url,
                    &err_item,
                )
                .await;
            }

            if let Ok(Some(updated)) = db.update_tracker_runtime(
                tracker_id,
                status,
                Some(&msg),
                Some(&now_str),
                Some(&next_str),
                &ctx.state,
            ) {
                let _ = event_tx.send(UiEvent::TrackerUpdated(updated.to_public()));
            }
        }
    }

    Ok(())
}

pub fn spawn_scheduler(
    db: Database,
    event_tx: broadcast::Sender<UiEvent>,
    rate_limiter: Arc<Mutex<Instant>>,
    notify_client: Client,
) -> SchedulerHandle {
    let (tx, mut rx) = mpsc::channel::<SchedulerCmd>(64);

    tokio::spawn(async move {
        let mut schedule: HashMap<String, DateTime<Utc>> = HashMap::new();

        let rebuild_schedule = |schedule: &mut HashMap<String, DateTime<Utc>>| {
            let Ok(trackers) = db.list_trackers() else {
                return;
            };
            let now = Utc::now();
            let mut next_map = HashMap::new();
            for t in trackers {
                if !t.enabled {
                    continue;
                }
                if let Some(existing) = schedule.get(&t.id) {
                    next_map.insert(t.id, *existing);
                } else if let Some(next_str) = &t.next_check_at
                    && let Ok(parsed) = DateTime::parse_from_rfc3339(next_str)
                {
                    next_map.insert(t.id, parsed.with_timezone(&Utc));
                } else {
                    next_map.insert(t.id, now);
                }
            }
            *schedule = next_map;
        };

        rebuild_schedule(&mut schedule);
        info!(active_trackers = schedule.len(), "Per-tracker scheduler started");

        loop {
            let now = Utc::now();
            let mut due_ids: Vec<String> = schedule
                .iter()
                .filter(|(_, next)| **next <= now)
                .map(|(id, _)| id.clone())
                .collect();
            due_ids.sort();

            for id in due_ids {
                if let Err(e) =
                    execute_tracker_check(&db, rate_limiter.clone(), &event_tx, &notify_client, &id)
                        .await
                {
                    warn!(tracker_id = %id, "Scheduled check error: {e}");
                }
                if let Ok(Some(t)) = db.get_tracker(&id)
                    && t.enabled
                {
                    let settings = db.get_settings().unwrap_or_default();
                    let secs = effective_interval_secs(&t, settings.default_check_interval_secs);
                    schedule.insert(id, Utc::now() + ChronoDuration::seconds(secs as i64));
                } else {
                    schedule.remove(&id);
                }
            }

            let sleep_dur = schedule
                .values()
                .min()
                .map(|next| {
                    let diff = (*next - Utc::now()).to_std().unwrap_or(Duration::from_secs(1));
                    diff.clamp(Duration::from_millis(250), Duration::from_secs(30))
                })
                .unwrap_or(Duration::from_secs(10));

            tokio::select! {
                _ = tokio::time::sleep(sleep_dur) => {}
                cmd = rx.recv() => {
                    match cmd {
                        Some(SchedulerCmd::Reload) => {
                            rebuild_schedule(&mut schedule);
                        }
                        Some(SchedulerCmd::RunNow(id)) => {
                            schedule.insert(id, Utc::now());
                        }
                        None => break,
                    }
                }
            }
        }
    });

    SchedulerHandle { tx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_patterns_match_substring_and_regex() {
        let item = NotificationItem {
            item_type: ItemType::Message,
            id: "1".into(),
            title: Some("Torrent Deleted".into()),
            subject: Some("Your class has changed to Power User".into()),
            sender: Some("System".into()),
            body: None,
            date: "2026-11-13".into(),
            url: "https://example.org".into(),
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        };

        assert!(should_ignore(&item, &["torrent deleted".into()]));
        assert!(should_ignore(&item, &["/power\\s+user/".into()]));
        assert!(!should_ignore(&item, &["warning".into()]));
    }
}
