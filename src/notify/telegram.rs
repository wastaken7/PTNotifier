use crate::models::NotificationItem;
use crate::notify::{
    escape_telegram_html, format_telegram_html, item_icon, split_telegram_html,
};
use anyhow::{Result, anyhow};
use reqwest::Client;
use serde_json::{Value, json};

/// Maximum `sendMessage` text length, in characters after entity parsing.
const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

pub async fn send(
    client: &Client,
    config: &Value,
    tracker_name: &str,
    item: &NotificationItem,
) -> Result<()> {
    let bot_token = config
        .get("bot_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Telegram bot_token is missing"))?;

    let chat_id = config
        .get("chat_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Telegram chat_id is missing"))?;

    let topic_id = config
        .get("topic_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let icon = item_icon(item.item_type);
    let mut text = format!(
        "<b>{}</b>

<b>{icon} New {}</b>

",
        escape_telegram_html(tracker_name),
        item.item_type.label()
    );

    if item.is_staff {
        text.push_str("⚠️ <b>STAFF MESSAGE</b> ⚠️

");
    }
    if let Some(sender) = &item.sender
        && !sender.is_empty()
    {
        text.push_str(&format!("👤 {}

", escape_telegram_html(sender)));
    }
    if let Some(title) = &item.title
        && !title.is_empty()
    {
        text.push_str(&format!("<b>Title:</b> {}

", escape_telegram_html(title)));
    }
    if let Some(subject) = &item.subject
        && !subject.is_empty()
    {
        text.push_str(&format!("<b>Subject:</b> {}

", escape_telegram_html(subject)));
    }
    if let Some(body) = &item.body
        && !body.is_empty()
    {
        let clean = format_telegram_html(body);
        if !clean.is_empty() {
            text.push_str(&format!("<b>Body:</b> {clean}

"));
        }
    }
    text.push_str(&escape_telegram_html(&item.date));

    let chunks: Vec<String> = split_telegram_html(&text, TELEGRAM_MAX_MESSAGE_LEN)
        .into_iter()
        .filter(|c| has_visible_text(c))
        .collect();
    let last_idx = chunks.len().saturating_sub(1);

    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    for (idx, chunk) in chunks.iter().enumerate() {
        let mut payload = json!({
            "chat_id": chat_id,
            "text": chunk,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });
        if idx == last_idx {
            payload["reply_markup"] = json!({
                "inline_keyboard": [[{"text": "Open Notification", "url": item.url}]]
            });
        }
        if let Some(tid) = topic_id {
            payload["message_thread_id"] = json!(tid);
        }

        // Telegram allows about one message per second per chat: pace multi-part messages
        // so later parts are not throttled.
        if idx > 0 {
            tokio::time::sleep(TELEGRAM_PART_GAP).await;
        }

        // On 429, wait as told and resend this same part, so a throttled multi-part message
        // resumes where it stopped instead of being retried (and duplicated) from part 1.
        let mut throttled = 0;
        loop {
            let resp = client.post(&url).json(&payload).send().await?;
            let status = resp.status();
            if status.is_success() {
                break;
            }
            let body = resp.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                && throttled < TELEGRAM_MAX_THROTTLE_RETRIES
            {
                throttled += 1;
                tokio::time::sleep(retry_after(&body)).await;
                continue;
            }
            return Err(anyhow!(
                "Telegram returned HTTP {status} (part {}/{}): {body}",
                idx + 1,
                chunks.len()
            ));
        }
    }
    Ok(())
}

const TELEGRAM_PART_GAP: std::time::Duration = std::time::Duration::from_millis(1100);
const TELEGRAM_MAX_THROTTLE_RETRIES: u32 = 3;
const TELEGRAM_MAX_RETRY_AFTER_SECS: u64 = 60;

/// Reads `parameters.retry_after` from a 429 body (falls back to a few seconds, capped).
fn retry_after(body: &str) -> std::time::Duration {
    let secs = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/parameters/retry_after")
                .and_then(|s| s.as_u64())
        })
        .unwrap_or(3);
    std::time::Duration::from_secs(secs.clamp(1, TELEGRAM_MAX_RETRY_AFTER_SECS))
}

/// Whether a chunk still has non-whitespace text once tags are removed (Telegram rejects empty messages).
fn has_visible_text(chunk: &str) -> bool {
    let mut in_tag = false;
    chunk.chars().any(|c| match c {
        '<' => {
            in_tag = true;
            false
        }
        '>' => {
            in_tag = false;
            false
        }
        _ => !in_tag && !c.is_whitespace(),
    })
}

#[cfg(test)]
mod tests {
    use super::retry_after;
    use std::time::Duration;

    #[test]
    fn retry_after_reads_and_caps_the_hint() {
        let body = r#"{"ok":false,"error_code":429,"parameters":{"retry_after":7}}"#;
        assert_eq!(retry_after(body), Duration::from_secs(7));
        assert_eq!(
            retry_after(r#"{"parameters":{"retry_after":9999}}"#),
            Duration::from_secs(60)
        );
        assert_eq!(retry_after("not json"), Duration::from_secs(3));
    }
}
