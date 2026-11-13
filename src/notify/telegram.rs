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

        let resp = client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Telegram returned HTTP {status} (part {}/{}): {body}",
                idx + 1,
                chunks.len()
            ));
        }
    }
    Ok(())
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
