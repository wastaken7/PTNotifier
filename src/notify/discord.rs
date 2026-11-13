use crate::models::{GlobalSettings, NotificationItem};
use crate::notify::{format_markdown, item_icon};
use anyhow::{Result, anyhow};
use reqwest::{Client, multipart};
use serde_json::{Value, json};
use std::io::Cursor;

const TRUNCATION_NOTICE: &str = "\n\n...[truncated]";

fn truncate_description(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    if max_len <= TRUNCATION_NOTICE.len() {
        return text.chars().take(max_len).collect();
    }
    let keep = max_len - TRUNCATION_NOTICE.len();
    let prefix: String = text.chars().take(keep).collect();
    format!("{}{TRUNCATION_NOTICE}", prefix.trim_end())
}

async fn download_png_favicon(client: &Client, icon_url: &str) -> Option<Vec<u8>> {
    let resp = client.get(icon_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

pub async fn send(
    client: &Client,
    config: &Value,
    settings: &GlobalSettings,
    tracker_name: &str,
    base_url: &str,
    item: &NotificationItem,
) -> Result<()> {
    let webhook_url = config
        .get("webhook_url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Discord webhook_url is missing"))?;

    let icon = item_icon(item.item_type);
    let mut desc = String::new();
    if item.is_staff {
        desc.push_str("⚠️ **STAFF MESSAGE** ⚠️\n\n");
    }
    if let Some(sender) = &item.sender
        && !sender.is_empty()
    {
        desc.push_str(&format!("👤 **{sender}**\n\n"));
    }
    if let Some(title) = &item.title
        && !title.is_empty()
    {
        desc.push_str(&format!("**Title:** {title}\n\n"));
    }
    if let Some(subject) = &item.subject
        && !subject.is_empty()
    {
        desc.push_str(&format!("**Subject:** {subject}\n\n"));
    }
    if let Some(body) = &item.body
        && !body.is_empty()
    {
        let clean = format_markdown(body);
        if !clean.is_empty() {
            desc.push_str(&format!("**Body:** {clean}\n\n"));
        }
    }

    let limit = settings.discord_embed_limit.max(256);
    let desc = truncate_description(desc.trim(), limit);

    let default_icon = format!("{}/favicon.ico", base_url.trim_end_matches('/'));
    let icon_url = item.favicon.as_deref().unwrap_or(&default_icon);
    let png_bytes = download_png_favicon(client, icon_url).await;

    let mut embed = json!({
        "title": format!("{icon} New {}", item.item_type.label()),
        "url": item.url,
        "description": desc,
        "color": if item.is_staff { 15158332 } else { 3447003 },
        "footer": { "text": item.date },
    });

    if png_bytes.is_some() {
        embed["author"] = json!({
            "name": tracker_name,
            "icon_url": "attachment://favicon.png",
        });
    } else {
        embed["author"] = json!({ "name": tracker_name });
    }

    let payload = json!({ "embeds": [embed] });

    let resp = if let Some(bytes) = png_bytes {
        let form = multipart::Form::new()
            .text("payload_json", payload.to_string())
            .part(
                "file",
                multipart::Part::bytes(bytes)
                    .file_name("favicon.png")
                    .mime_str("image/png")?,
            );
        client.post(webhook_url).multipart(form).send().await?
    } else {
        client.post(webhook_url).json(&payload).send().await?
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Discord returned HTTP {status}: {text}"));
    }
    Ok(())
}
