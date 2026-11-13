pub mod discord;
pub mod gotify;
pub mod ntfy;
pub mod telegram;

use crate::models::{ChannelConfig, ChannelKind, GlobalSettings, ItemType, NotificationItem};
use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use std::sync::OnceLock;
use tracing::error;

pub async fn dispatch_item(
    client: &Client,
    channels: &[ChannelConfig],
    settings: &GlobalSettings,
    tracker_name: &str,
    base_url: &str,
    item: &NotificationItem,
) {
    for ch in channels {
        if !ch.enabled {
            continue;
        }
        let should_send = match item.item_type {
            ItemType::Notification => ch.notify_notifications,
            ItemType::Message => ch.notify_pms,
            ItemType::Error => ch.notify_errors,
        };
        if !should_send {
            continue;
        }

        if let Err(e) = send_to_channel(client, ch, settings, tracker_name, base_url, item).await {
            error!(channel = %ch.name, kind = %ch.kind.as_str(), "Failed to send notification: {e}");
        }
    }
}

pub async fn send_to_channel(
    client: &Client,
    channel: &ChannelConfig,
    settings: &GlobalSettings,
    tracker_name: &str,
    base_url: &str,
    item: &NotificationItem,
) -> Result<()> {
    match channel.kind {
        ChannelKind::Discord => {
            discord::send(client, &channel.config, settings, tracker_name, base_url, item).await
        }
        ChannelKind::Telegram => {
            telegram::send(client, &channel.config, tracker_name, item).await
        }
        ChannelKind::Gotify => {
            gotify::send(client, &channel.config, tracker_name, item).await
        }
        ChannelKind::Ntfy => {
            ntfy::send(client, &channel.config, tracker_name, item).await
        }
    }
}

/// Converts BBCode and HTML tags into clean Markdown suitable for Discord/Gotify/ntfy.
pub fn format_markdown(raw: &str) -> String {
    static RE_BOLD: OnceLock<Regex> = OnceLock::new();
    static RE_ITALIC: OnceLock<Regex> = OnceLock::new();
    static RE_UNDER: OnceLock<Regex> = OnceLock::new();
    static RE_STRIKE: OnceLock<Regex> = OnceLock::new();
    static RE_URL_BB: OnceLock<Regex> = OnceLock::new();
    static RE_URL_HTML: OnceLock<Regex> = OnceLock::new();
    static RE_CODE: OnceLock<Regex> = OnceLock::new();
    static RE_BR: OnceLock<Regex> = OnceLock::new();
    static RE_HTML_TAG: OnceLock<Regex> = OnceLock::new();
    static RE_BBCODE: OnceLock<Regex> = OnceLock::new();

    let re_bold = RE_BOLD.get_or_init(|| {
        Regex::new(r"(?is)\[b\](.*?)\[/b\]|<b>(.*?)</b>|<strong>(.*?)</strong>").unwrap()
    });
    let re_italic = RE_ITALIC.get_or_init(|| {
        Regex::new(r"(?is)\[i\](.*?)\[/i\]|<i>(.*?)</i>|<em>(.*?)</em>").unwrap()
    });
    let re_under = RE_UNDER
        .get_or_init(|| Regex::new(r"(?is)\[u\](.*?)\[/u\]|<u>(.*?)</u>").unwrap());
    let re_strike = RE_STRIKE.get_or_init(|| {
        Regex::new(r"(?is)\[s\](.*?)\[/s\]|<s>(.*?)</s>|<strike>(.*?)</strike>").unwrap()
    });
    let re_url_bb = RE_URL_BB
        .get_or_init(|| Regex::new(r"(?is)\[url=(.*?)\](.*?)\[/url\]").unwrap());
    let re_url_html = RE_URL_HTML.get_or_init(|| {
        Regex::new(r#"(?is)<a\s+[^>]*href=["'](.*?)["'][^>]*>(.*?)</a>"#).unwrap()
    });
    let re_code = RE_CODE
        .get_or_init(|| Regex::new(r"(?is)\[code\](.*?)\[/code\]|<code>(.*?)</code>").unwrap());
    let re_br = RE_BR.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    let re_html = RE_HTML_TAG.get_or_init(|| Regex::new(r"<[^>]+>").unwrap());
    let re_bb = RE_BBCODE.get_or_init(|| {
        Regex::new(r"(?i)\[/?(?:b|i|u|s|url|code|quote|spoiler|color|size|font|align|center|left|right|img|list|hr|\*)(?:=[^\]]*)?\]").unwrap()
    });

    let s = re_br.replace_all(raw, "\n");
    let s = re_bold.replace_all(&s, |c: &regex::Captures| {
        let inner = c.get(1).or(c.get(2)).or(c.get(3)).map_or("", |m| m.as_str());
        format!("**{inner}**")
    });
    let s = re_italic.replace_all(&s, |c: &regex::Captures| {
        let inner = c.get(1).or(c.get(2)).or(c.get(3)).map_or("", |m| m.as_str());
        format!("*{inner}*")
    });
    let s = re_under.replace_all(&s, |c: &regex::Captures| {
        let inner = c.get(1).or(c.get(2)).map_or("", |m| m.as_str());
        format!("__{inner}__")
    });
    let s = re_strike.replace_all(&s, |c: &regex::Captures| {
        let inner = c.get(1).or(c.get(2)).or(c.get(3)).map_or("", |m| m.as_str());
        format!("~~{inner}~~")
    });
    let link_fmt = |url: &str, text: &str| {
        let url = url.trim();
        let text = text.trim();
        if text.is_empty()
            || text == url
            || text.to_lowercase().starts_with("http://")
            || text.to_lowercase().starts_with("https://")
        {
            url.to_string()
        } else {
            format!("[{text}]({url})")
        }
    };
    let s = re_url_bb.replace_all(&s, |c: &regex::Captures| link_fmt(&c[1], &c[2]));
    let s = re_url_html.replace_all(&s, |c: &regex::Captures| link_fmt(&c[1], &c[2]));
    let s = re_code.replace_all(&s, |c: &regex::Captures| {
        let inner = c.get(1).or(c.get(2)).map_or("", |m| m.as_str());
        format!("`{inner}`")
    });
    let s = re_html.replace_all(&s, "");
    let s = re_bb.replace_all(&s, "");
    s.trim().to_string()
}

/// Converts BBCode and generic HTML to Telegram-compatible HTML.
pub fn format_telegram_html(raw: &str) -> String {
    static RE_B: OnceLock<Regex> = OnceLock::new();
    static RE_I: OnceLock<Regex> = OnceLock::new();
    static RE_U: OnceLock<Regex> = OnceLock::new();
    static RE_S: OnceLock<Regex> = OnceLock::new();
    static RE_SPOILER: OnceLock<Regex> = OnceLock::new();
    static RE_URL: OnceLock<Regex> = OnceLock::new();
    static RE_CODE: OnceLock<Regex> = OnceLock::new();
    static RE_BR: OnceLock<Regex> = OnceLock::new();
    static RE_STRIKE_HTML: OnceLock<Regex> = OnceLock::new();

    let s = RE_BR
        .get_or_init(|| Regex::new(r"(?i)<br\s*/?>").unwrap())
        .replace_all(raw, "\n");
    let s = RE_B
        .get_or_init(|| Regex::new(r"(?is)\[b\](.*?)\[/b\]").unwrap())
        .replace_all(&s, "<b>$1</b>");
    let s = RE_I
        .get_or_init(|| Regex::new(r"(?is)\[i\](.*?)\[/i\]").unwrap())
        .replace_all(&s, "<i>$1</i>");
    let s = RE_U
        .get_or_init(|| Regex::new(r"(?is)\[u\](.*?)\[/u\]").unwrap())
        .replace_all(&s, "<u>$1</u>");
    let s = RE_S
        .get_or_init(|| Regex::new(r"(?is)\[s\](.*?)\[/s\]").unwrap())
        .replace_all(&s, "<s>$1</s>");
    let s = RE_SPOILER
        .get_or_init(|| Regex::new(r"(?is)\[spoiler\](.*?)\[/spoiler\]|<spoiler>(.*?)</spoiler>").unwrap())
        .replace_all(&s, |c: &regex::Captures| {
            let inner = c.get(1).or(c.get(2)).map_or("", |m| m.as_str());
            format!("<tg-spoiler>{inner}</tg-spoiler>")
        });
    let s = RE_URL
        .get_or_init(|| Regex::new(r"(?is)\[url=(.*?)\](.*?)\[/url\]").unwrap())
        .replace_all(&s, r#"<a href="$1">$2</a>"#);
    let s = RE_CODE
        .get_or_init(|| Regex::new(r"(?is)\[code\](.*?)\[/code\]").unwrap())
        .replace_all(&s, "<code>$1</code>");
    let s = RE_STRIKE_HTML
        .get_or_init(|| Regex::new(r"(?is)<strike>(.*?)</strike>").unwrap())
        .replace_all(&s, "<s>$1</s>");

    s.trim().to_string()
}

pub fn item_icon(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Notification => "🔔",
        ItemType::Message => "📩",
        ItemType::Error => "🚨",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bbcode_and_html_to_markdown() {
        let input = "[b]Hello[/b] <a href=\"https://example.com\">Click</a> [code]x=1[/code]";
        let md = format_markdown(input);
        assert_eq!(md, "**Hello** [Click](https://example.com) `x=1`");
    }

    #[test]
    fn converts_bbcode_to_telegram_html() {
        let input = "[b]Alert[/b] [spoiler]secret[/spoiler]";
        let html = format_telegram_html(input);
        assert_eq!(html, "<b>Alert</b> <tg-spoiler>secret</tg-spoiler>");
    }
}
