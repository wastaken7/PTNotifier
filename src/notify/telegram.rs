use crate::models::NotificationItem;
use crate::notify::{format_telegram_html, item_icon};
use anyhow::{Result, anyhow};
use reqwest::Client;
use serde_json::{Value, json};

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
        "<b>{tracker_name}</b>\n\n<b>{icon} New {}</b>\n\n",
        item.item_type.label()
    );

    if item.is_staff {
        text.push_str("⚠️ <b>STAFF MESSAGE</b> ⚠️\n\n");
    }
    if let Some(sender) = &item.sender
        && !sender.is_empty()
    {
        text.push_str(&format!("👤 {sender}\n\n"));
    }
    if let Some(title) = &item.title
        && !title.is_empty()
    {
        text.push_str(&format!("<b>Title:</b> {title}\n\n"));
    }
    if let Some(subject) = &item.subject
        && !subject.is_empty()
    {
        text.push_str(&format!("<b>Subject:</b> {subject}\n\n"));
    }
    if let Some(body) = &item.body
        && !body.is_empty()
    {
        let clean = format_telegram_html(body);
        if !clean.is_empty() {
            text.push_str(&format!("<b>Body:</b> {clean}\n\n"));
        }
    }
    text.push_str(&item.date);

    let mut payload = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true,
        "reply_markup": {
            "inline_keyboard": [[{"text": "Open Notification", "url": item.url}]]
        }
    });

    if let Some(tid) = topic_id {
        payload["message_thread_id"] = json!(tid);
    }

    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let resp = client.post(&url).json(&payload).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Telegram returned HTTP {status}: {body}"));
    }
    Ok(())
}
