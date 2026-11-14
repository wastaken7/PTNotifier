use crate::models::NotificationItem;
use crate::notify::{format_markdown, item_icon};
use anyhow::{Result, anyhow};
use reqwest::Client;
use serde_json::{Value, json};

pub async fn send(
    client: &Client,
    config: &Value,
    tracker_name: &str,
    item: &NotificationItem,
) -> Result<()> {
    let base_url = config
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().trim_end_matches('/'))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Gotify url is missing"))?;

    let token = config
        .get("token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Gotify token is missing"))?;

    let icon = item_icon(item.item_type);
    let title = format!("{tracker_name} - {icon} New {}", item.item_type.label());

    let mut content = String::new();
    if item.is_staff {
        content.push_str("⚠️ **STAFF MESSAGE** ⚠️\n\n");
    }
    if let Some(sender) = &item.sender
        && !sender.is_empty()
    {
        content.push_str(&format!("👤 {sender}\n\n"));
    }
    if let Some(t) = &item.title
        && !t.is_empty()
    {
        content.push_str(&format!("**Title:** {t}\n\n"));
    }
    if let Some(subject) = &item.subject
        && !subject.is_empty()
    {
        content.push_str(&format!("**Subject:** {subject}\n\n"));
    }
    if let Some(body) = &item.body
        && !body.is_empty()
    {
        let clean = format_markdown(body);
        if !clean.is_empty() {
            content.push_str(&format!("**Body:** {clean}\n\n"));
        }
    }
    content.push_str(&format!(
        "**Date:** {}\n\n[Open Notification]({})",
        item.date, item.url
    ));

    let payload = json!({
        "title": title,
        "message": content,
        "priority": 5,
        "extras": {
            "client::display": { "contentType": "text/markdown" }
        }
    });

    let url = format!("{base_url}/message?token={token}");
    let resp = client.post(&url).json(&payload).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Gotify returned HTTP {status}: {body}"));
    }
    Ok(())
}
