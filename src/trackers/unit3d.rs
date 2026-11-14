use crate::models::{ItemType, NotificationItem};
use crate::trackers::{TrackerContext, TrackerError, sel, text_clean};
use scraper::Html;
use tracing::warn;

/// Stable short digest for IDs that are persisted across runs and toolchain upgrades
/// (`DefaultHasher` output is not guaranteed to be stable between Rust releases).
fn short_hash(s: &str) -> String {
    crate::security::sha256_hex(s)[..16].to_string()
}

async fn ensure_initialized(ctx: &mut TrackerContext, hawke_uno: bool) -> Result<(), TrackerError> {
    if hawke_uno {
        ctx.state.notifications_url = Some(ctx.join_url("/notifications"));
        ctx.state.messages_url = Some(ctx.join_url("/mail/inbox"));
    }

    if ctx.state.notifications_url.is_some() && ctx.state.messages_url.is_some() && !hawke_uno {
        return Ok(());
    }
    if hawke_uno && ctx.state.username.is_some() && ctx.state.csrf_token.is_some() {
        return Ok(());
    }

    let html = ctx.fetch_page(&ctx.config.base_url, "initialize", None).await?;
    let doc = Html::parse_document(&html);

    if let Some(meta) = doc.select(&sel(r#"meta[name="csrf-token"]"#)).next()
        && let Some(token) = meta.value().attr("content")
    {
        ctx.state.csrf_token = Some(token.to_string());
    }

    if hawke_uno {
        if let Some(user_el) = doc.select(&sel("a.deep-space-user-card__header-username")).next() {
            let username = text_clean(user_el);
            if !username.is_empty() {
                ctx.state.username = Some(username);
            }
        }
        return Ok(());
    }

    for a in doc.select(&sel("a[href]")) {
        if let Some(href) = a.value().attr("href") {
            if ctx.state.notifications_url.is_none() && href.contains("notifications") {
                ctx.state.notifications_url = Some(ctx.join_url(href));
            }
            if ctx.state.messages_url.is_none() && href.contains("conversations") {
                ctx.state.messages_url = Some(ctx.join_url(href));
            }
        }
    }

    // A login page has neither link: report it instead of silently checking nothing.
    let Some(nurl) = ctx.state.notifications_url.clone() else {
        ctx.state.messages_url = None;
        return Err(TrackerError::CookieExpired(
            "initialize: notifications link not found (expired cookies or HTML changed)".into(),
        ));
    };
    if ctx.state.messages_url.is_none() {
        ctx.state.messages_url = Some(nurl.replace("notifications", "conversations"));
    }

    Ok(())
}

pub async fn fetch_items(
    ctx: &mut TrackerContext,
    hawke_uno: bool,
) -> Result<Vec<NotificationItem>, TrackerError> {
    ensure_initialized(ctx, hawke_uno).await?;

    let mut items = Vec::new();

    if let Some(notif_url) = ctx.state.notifications_url.clone() {
        let html = ctx
            .fetch_page(&notif_url, "notifications", Some("/notifications"))
            .await?;
        let parsed = if hawke_uno {
            parse_hawke_notifications(&html, ctx)
        } else {
            parse_unit3d_notifications(&html, ctx)
        };
        items.extend(parsed);
    }

    if let Some(msg_url) = ctx.state.messages_url.clone() {
        let html = ctx
            .fetch_page(&msg_url, "messages", Some("/notifications"))
            .await?;
        let mut msgs = if hawke_uno {
            parse_hawke_messages(&html, ctx)
        } else {
            let (msgs, migrated) = parse_unit3d_messages(&html, ctx);
            for id in migrated {
                ctx.mark_processed(&id);
            }
            msgs
        };
        // Opening a UNIT3D conversation marks it read, so only fetch bodies when the user
        // asked for that, and never while seeding (seeded items aren't delivered).
        if !hawke_uno && ctx.state.initialized && ctx.should_mark_as_read() {
            for m in &mut msgs {
                if let Ok(body_html) = ctx.fetch_page(&m.url, "message body", None).await {
                    let doc = Html::parse_document(&body_html);
                    if let Some(last) = doc.select(&sel("div.panel__body.bbcode-rendered")).last() {
                        m.body = Some(last.html());
                    }
                }
            }
        }
        items.extend(msgs);
    }

    Ok(items)
}

fn parse_unit3d_notifications(html: &str, ctx: &TrackerContext) -> Vec<NotificationItem> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();

    for row in doc.select(&sel("tr")) {
        if row.select(&sel("td.notification--unread")).next().is_none() {
            continue;
        }
        let cols: Vec<_> = row.select(&sel("td")).collect();
        if cols.len() < 4 {
            continue;
        }
        let Some(form) = cols[3].select(&sel("form")).next() else {
            continue;
        };
        let Some(action) = form.value().attr("action") else {
            continue;
        };
        let action_abs = ctx.join_url(action);
        let id_part = action_abs
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&action_abs);
        let notif_id = format!("notif_{id_part}");
        if ctx.is_processed(&notif_id) {
            continue;
        }

        items.push(NotificationItem {
            item_type: ItemType::Notification,
            id: notif_id,
            title: Some(text_clean(cols[0])),
            subject: Some(text_clean(cols[1])),
            sender: None,
            body: None,
            date: text_clean(cols[2]),
            url: action_abs.clone(),
            is_staff: false,
            favicon: None,
            mark_read_url: Some(action_abs),
        });
    }
    items
}

/// Unread conversations, plus the new-style IDs of conversations already seen under a
/// legacy (bare) ID, which the caller marks processed.
fn parse_unit3d_messages(html: &str, ctx: &TrackerContext) -> (Vec<NotificationItem>, Vec<String>) {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();
    let mut migrated = Vec::new();

    for row in doc.select(&sel("tr")) {
        if row.select(&sel("i.text-red")).next().is_none() {
            continue;
        }
        let cols: Vec<_> = row.select(&sel("td")).collect();
        if cols.len() < 6 {
            continue;
        }
        let sender = text_clean(cols[0]);
        let Some(link) = cols[1].select(&sel("a[href]")).next() else {
            continue;
        };
        let subject = text_clean(link);
        let href = link.value().attr("href").unwrap_or_default();
        let abs_url = ctx.join_url(href);
        let id_part = abs_url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&abs_url);
        // A conversation keeps its URL across replies, so key each update by the
        // conversation plus its reply count (or, failing that, its updated_at timestamp):
        // a new reply yields a new item instead of being skipped forever.
        let version = Some(text_clean(cols[3]))
            .filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_digit()))
            .or_else(|| {
                cols[2]
                    .select(&sel("time[datetime]"))
                    .next()
                    .and_then(|t| t.value().attr("datetime"))
                    .map(str::to_string)
            });
        let legacy_id = format!("msg_{id_part}");
        let msg_id = match &version {
            Some(v) => format!("{legacy_id}@{v}"),
            None => legacy_id.clone(),
        };
        if ctx.is_processed(&msg_id) {
            continue;
        }
        // Older versions stored the bare conversation ID: treat that as this version
        // already seen (it is re-keyed by `fetch_items`), rather than re-notifying every
        // unread conversation once after upgrading.
        if msg_id != legacy_id && ctx.is_processed(&legacy_id) {
            migrated.push(msg_id);
            continue;
        }

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: msg_id,
            title: None,
            subject: Some(subject),
            sender: Some(sender),
            body: None,
            date: text_clean(cols[2]),
            url: abs_url,
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }
    (items, migrated)
}

fn parse_hawke_notifications(html: &str, ctx: &TrackerContext) -> Vec<NotificationItem> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();

    for row in doc.select(&sel("tr.ds-macro-row--unread")) {
        let Some(name_td) = row.select(&sel("td.ds-macro-row__name")).next() else {
            continue;
        };
        let Some(link) = name_td.select(&sel("a[href]")).next() else {
            continue;
        };
        let title = text_clean(link);
        let href = link.value().attr("href").unwrap_or_default();
        let cols: Vec<_> = row.select(&sel("td")).collect();
        let date = cols.get(1).map(|c| text_clean(*c)).unwrap_or_default();

        let wire_key = row.value().attr("wire:key").unwrap_or_default();
        let notif_id = if wire_key.is_empty() {
            format!("notif_{}", short_hash(&title))
        } else {
            wire_key.to_string()
        };
        if ctx.is_processed(&notif_id) {
            continue;
        }

        items.push(NotificationItem {
            item_type: ItemType::Notification,
            id: notif_id,
            title: Some(title),
            subject: None,
            sender: None,
            body: None,
            date,
            url: ctx.join_url(href),
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }
    items
}

fn parse_hawke_messages(html: &str, ctx: &TrackerContext) -> Vec<NotificationItem> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();
    let username = ctx.state.username.as_deref().unwrap_or("me");

    for row in doc.select(&sel("div.deep-space-messages__row")) {
        let Some(user_el) = row.select(&sel("span.deep-space-messages__row-username")).next() else {
            continue;
        };
        let sender = text_clean(user_el);
        let subject = row
            .select(&sel("div.deep-space-messages__row-preview"))
            .next()
            .map(text_clean)
            .unwrap_or_else(|| "New Message".into());
        let wire_key = row.value().attr("wire:key").unwrap_or_default();
        let corr_id = wire_key.rsplit('-').next().unwrap_or("");
        // Rows are per correspondent, so `corr_id` alone would suppress every later message
        // from the same person. The latest message's preview identifies the message itself.
        let item_id = format!("msg_{corr_id}_{}", short_hash(&format!("{sender}
{subject}")));
        if ctx.is_processed(&item_id) {
            continue;
        }

        let mut msg_url = ctx.join_url(&format!("/users/{username}/hub/messages"));
        if !corr_id.is_empty() {
            msg_url.push_str(&format!("?activeCorrespondentId={corr_id}"));
        }
        let date = row
            .select(&sel("span.deep-space-messages__row-time"))
            .next()
            .map(text_clean)
            .unwrap_or_default();

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id,
            title: None,
            subject: Some(subject),
            sender: Some(sender),
            body: None,
            date,
            url: msg_url,
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }
    items
}

pub async fn mark_notification_read(ctx: &TrackerContext, action_url: &str) {
    let Some(csrf) = &ctx.state.csrf_token else {
        return;
    };
    // `action_url` comes from the tracker's HTML.
    if let Err(e) = ctx.check_outbound(action_url, "mark as read") {
        warn!(tracker = %ctx.config.name, "{e}");
        return;
    }
    let referer = ctx
        .state
        .notifications_url
        .as_deref()
        .unwrap_or(&ctx.config.base_url);
    let res = ctx
        .client
        .post(action_url)
        .header("Referer", referer)
        .form(&[("_token", csrf.as_str()), ("_method", "PATCH")])
        .send()
        .await;
    if let Err(e) = res {
        warn!(tracker = %ctx.config.name, "Failed to mark UNIT3D notification as read: {}", e.without_url());
    }
}

#[cfg(test)]
mod conversation_tests {
    use super::parse_unit3d_messages;
    use crate::models::{GlobalSettings, TrackerConfig, TrackerState, TrackerStatus};
    use crate::trackers::TrackerContext;
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::Mutex;

    fn ctx(processed: &[&str]) -> TrackerContext {
        let config = TrackerConfig {
            id: "t".into(),
            name: "T".into(),
            kind: "unit3d".into(),
            base_url: "https://u3d.example".into(),
            enabled: true,
            check_interval_secs: None,
            mark_as_read: None,
            cookies_raw: String::new(),
            api_token: String::new(),
            ignore_patterns: vec![],
            state: TrackerState {
                processed_ids: processed.iter().map(|s| s.to_string()).collect(),
                initialized: true,
                ..Default::default()
            },
            last_status: TrackerStatus::Ok,
            last_error: None,
            last_checked_at: None,
            next_check_at: None,
            created_at: String::new(),
            config_rev: 0,
        };
        TrackerContext::new(
            config,
            GlobalSettings::default(),
            Arc::new(Mutex::new(Instant::now())),
        )
        .unwrap()
    }

    fn inbox(replies: u32) -> String {
        format!(
            r#"<table><tr>
                <td>alice</td>
                <td><a href="https://u3d.example/users/me/conversations/42">Hello</a></td>
                <td><time datetime="2026-11-13 10:00:00">1 hour ago</time></td>
                <td>{replies}</td>
                <td><i class="fas fa-times text-red" title="unread"></i></td>
                <td></td>
            </tr></table>"#
        )
    }

    #[test]
    fn a_new_reply_in_a_seen_conversation_is_a_new_item() {
        let (items, migrated) = parse_unit3d_messages(&inbox(3), &ctx(&["msg_42@2"]));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "msg_42@3");
        assert!(migrated.is_empty());

        let (items, _) = parse_unit3d_messages(&inbox(3), &ctx(&["msg_42@3"]));
        assert!(items.is_empty());
    }

    #[test]
    fn legacy_conversation_ids_are_migrated_without_renotifying() {
        let (items, migrated) = parse_unit3d_messages(&inbox(3), &ctx(&["msg_42"]));
        assert!(items.is_empty());
        assert_eq!(migrated, vec!["msg_42@3".to_string()]);
    }
}
