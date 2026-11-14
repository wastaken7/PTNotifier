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

/// Sends `item` to every enabled channel subscribed to its type and returns the IDs of the
/// channels whose delivery failed. When `only` is given, delivery is restricted to those
/// channel IDs (used to retry previously failed deliveries).
pub async fn dispatch_item(
    client: &Client,
    channels: &[ChannelConfig],
    settings: &GlobalSettings,
    tracker_name: &str,
    base_url: &str,
    item: &NotificationItem,
    only: Option<&[String]>,
) -> Vec<String> {
    let mut failed = Vec::new();
    for ch in channels {
        if !ch.enabled {
            continue;
        }
        if only.is_some_and(|ids| !ids.contains(&ch.id)) {
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
            failed.push(ch.id.clone());
        }
    }
    failed
}

pub async fn send_to_channel(
    client: &Client,
    channel: &ChannelConfig,
    settings: &GlobalSettings,
    tracker_name: &str,
    base_url: &str,
    item: &NotificationItem,
) -> Result<()> {
    send_to_channel_inner(client, channel, settings, tracker_name, base_url, item)
        .await
        .map_err(redact_transport_error)
}

/// Channel URLs embed credentials (Gotify `?token=`, Telegram `/bot<token>/`, Discord webhook
/// secrets), and `reqwest::Error`'s message includes the request URL. Strip it before the error
/// reaches the logs, which are retained and broadcast to the WebUI.
fn redact_transport_error(err: anyhow::Error) -> anyhow::Error {
    match err.downcast::<reqwest::Error>() {
        Ok(re) => anyhow::Error::new(re.without_url()),
        Err(other) => other,
    }
}

async fn send_to_channel_inner(
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
    // Decode last, in a single pass: tags are already gone, so `&lt;b&gt;` becomes literal
    // text instead of being stripped as markup, and `&amp;lt;` stays `&lt;`.
    decode_html_entities(s.trim())
}

/// Single-pass decoding of HTML character references (common named ones and numeric).
/// Unknown named entities are left untouched.
pub fn decode_html_entities(raw: &str) -> String {
    static RE_ENT: OnceLock<Regex> = OnceLock::new();
    let re = RE_ENT
        .get_or_init(|| Regex::new(r"&(#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z]{2,8});").unwrap());
    re.replace_all(raw, |c: &regex::Captures| {
        let ent = &c[1];
        let decoded = if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = ent.strip_prefix('#') {
            dec.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => None,
            }
        };
        decoded.map_or_else(|| c[0].to_string(), String::from)
    })
    .into_owned()
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

    sanitize_telegram_html(s.trim())
}

/// Escapes plain text for Telegram's HTML parse mode.
pub fn escape_telegram_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Maps an HTML tag name to the Telegram-supported tag it should be rendered as.
fn telegram_tag(name: &str) -> Option<&'static str> {
    Some(match name {
        "b" | "strong" => "b",
        "i" | "em" => "i",
        "u" | "ins" => "u",
        "s" | "strike" | "del" => "s",
        "code" => "code",
        "pre" => "pre",
        "a" => "a",
        "tg-spoiler" => "tg-spoiler",
        "blockquote" => "blockquote",
        _ => return None,
    })
}

/// Appends `text` to `out`, escaping markup characters. Entities Telegram understands are kept;
/// a few common named entities are decoded; any other `&` is escaped.
fn push_telegram_text(out: &mut String, text: &str) {
    static RE_ENTITY: OnceLock<Regex> = OnceLock::new();
    let re_entity = RE_ENTITY
        .get_or_init(|| Regex::new(r"^&(?:#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z]{2,8});").unwrap());

    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => {
                if let Some(m) = re_entity.find(&text[i..]) {
                    let ent = m.as_str();
                    let replacement = match ent {
                        "&lt;" | "&gt;" | "&amp;" | "&quot;" => Some(ent),
                        "&nbsp;" => Some(" "),
                        "&apos;" => Some("'"),
                        _ if ent.starts_with("&#") => Some(ent),
                        _ => None,
                    };
                    if let Some(r) = replacement {
                        out.push_str(r);
                        i += ent.len();
                        continue;
                    }
                }
                out.push_str("&amp;");
            }
            _ => out.push(c),
        }
        i += c.len_utf8();
    }
}

fn decode_basic_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Reduces arbitrary HTML to the subset Telegram accepts: supported tags are normalized and
/// kept balanced, unsupported tags are dropped, and text is escaped.
pub fn sanitize_telegram_html(input: &str) -> String {
    static RE_TAG: OnceLock<Regex> = OnceLock::new();
    static RE_HREF: OnceLock<Regex> = OnceLock::new();
    let re_tag = RE_TAG.get_or_init(|| Regex::new(r"(?s)<(/?)([a-zA-Z][a-zA-Z0-9-]*)([^<>]*)>").unwrap());
    let re_href = RE_HREF.get_or_init(|| {
        Regex::new(r#"(?is)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#).unwrap()
    });

    let mut out = String::with_capacity(input.len());
    let mut stack: Vec<&'static str> = Vec::new();
    let mut last = 0;

    for caps in re_tag.captures_iter(input) {
        let whole = caps.get(0).unwrap();
        push_telegram_text(&mut out, &input[last..whole.start()]);
        last = whole.end();

        let closing = !caps[1].is_empty();
        let Some(tag) = telegram_tag(&caps[2].to_ascii_lowercase()) else {
            continue;
        };

        if closing {
            if let Some(pos) = stack.iter().rposition(|t| *t == tag) {
                while stack.len() > pos {
                    let t = stack.pop().unwrap();
                    out.push_str("</");
                    out.push_str(t);
                    out.push('>');
                }
            }
            continue;
        }

        // Telegram does not allow formatting inside code blocks (except <code> in <pre>),
        // nor nested links.
        let in_code = stack.iter().any(|t| *t == "code" || *t == "pre");
        if in_code && !(tag == "code" && stack.last() == Some(&"pre")) {
            continue;
        }
        if tag == "a" {
            if stack.contains(&"a") {
                continue;
            }
            let href = re_href.captures(&caps[3]).and_then(|h| {
                h.get(1).or(h.get(2)).or(h.get(3)).map(|m| decode_basic_entities(m.as_str().trim()))
            });
            let Some(href) = href.filter(|h| {
                let lower = h.to_ascii_lowercase();
                lower.starts_with("http://") || lower.starts_with("https://")
            }) else {
                continue;
            };
            out.push_str("<a href=\"");
            out.push_str(&escape_telegram_html(&href));
            out.push_str("\">");
        } else {
            out.push('<');
            out.push_str(tag);
            out.push('>');
        }
        stack.push(tag);
    }
    push_telegram_text(&mut out, &input[last..]);
    while let Some(t) = stack.pop() {
        out.push_str("</");
        out.push_str(t);
        out.push('>');
    }
    out
}

/// Splits sanitized Telegram HTML into chunks of at most `limit` visible characters (counted in
/// UTF-16 code units after entity parsing, as Telegram does). Formatting tags that are open at a
/// split point are closed at the end of the chunk and reopened at the start of the next one.
pub fn split_telegram_html(html: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(1);
    let soft_limit = limit.saturating_sub(512);
    let mut chunks = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    let mut open: Vec<(String, String)> = Vec::new();

    let flush = |cur: &mut String, cur_len: &mut usize, open: &[(String, String)], chunks: &mut Vec<String>| {
        for (_, name) in open.iter().rev() {
            cur.push_str("</");
            cur.push_str(name);
            cur.push('>');
        }
        chunks.push(std::mem::take(cur));
        *cur_len = 0;
        for (tag, _) in open {
            cur.push_str(tag);
        }
    };

    let mut i = 0;
    while i < html.len() {
        let rest = &html[i..];
        if rest.starts_with('<') {
            let end = rest.find('>').map_or(rest.len(), |e| e + 1);
            let tag = &rest[..end];
            if tag.starts_with("</") {
                open.pop();
            } else {
                let name: String = tag[1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect();
                open.push((tag.to_string(), name));
            }
            cur.push_str(tag);
            i += end;
            continue;
        }

        // Look for an entity's ';' only after '&', and only within entity length, so plain
        // text isn't rescanned to the end on every character (quadratic on long messages).
        let entity_end = if rest.starts_with('&') {
            rest.as_bytes().iter().take(11).position(|&b| b == b';')
        } else {
            None
        };
        let (unit, width) = match entity_end {
            Some(semi) => (&rest[..=semi], 1),
            _ => {
                let c = rest.chars().next().unwrap();
                (&rest[..c.len_utf8()], c.len_utf16())
            }
        };

        if cur_len > 0 && cur_len + width > limit {
            flush(&mut cur, &mut cur_len, &open, &mut chunks);
        }
        cur.push_str(unit);
        cur_len += width;
        i += unit.len();

        // Prefer splitting on a line break once the chunk is nearly full.
        if unit == "\n" && cur_len >= soft_limit && soft_limit > 0 {
            flush(&mut cur, &mut cur_len, &open, &mut chunks);
        }
    }
    if cur_len > 0 {
        chunks.push(cur);
    }
    chunks
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

    #[test]
    fn markdown_decodes_entities_in_text_and_links() {
        let md = format_markdown(
            r#"A &amp; B &lt;tag&gt; &amp;lt; <a href="https://e.org/?a=1&amp;b=2">x</a> &#39;q&#x27; &copy;"#,
        );
        assert_eq!(md, "A & B <tag> &lt; [x](https://e.org/?a=1&b=2) 'q' &copy;");
    }

    #[test]
    fn telegram_html_escapes_text_and_keeps_only_supported_balanced_tags() {
        assert_eq!(escape_telegram_html("Use <name> & co"), "Use &lt;name&gt; &amp; co");

        let html = sanitize_telegram_html(
            r#"<div class="x">a < b &amp; c &nbsp;&copy; <strong>bold <em>it</strong> <a href="javascript:alert(1)">bad</a> <a href='https://e.org/?a=1&amp;b=2'>ok</a> <b>open"#,
        );
        assert_eq!(
            html,
            r#"a &lt; b &amp; c  &amp;copy; <b>bold <i>it</i></b> bad <a href="https://e.org/?a=1&amp;b=2">ok</a> <b>open</b>"#
        );

        // No formatting inside code blocks.
        assert_eq!(
            sanitize_telegram_html("<code><b>x</b></code>"),
            "<code>x</code>"
        );
    }

    #[test]
    fn telegram_split_respects_limit_and_reopens_tags() {
        let text = format!("<b>{}</b>", "a".repeat(10));
        let chunks = split_telegram_html(&text, 4);
        assert_eq!(chunks, vec!["<b>aaaa</b>", "<b>aaaa</b>", "<b>aa</b>"]);

        // Entities count as one character.
        let chunks = split_telegram_html("&amp;&amp;&amp;", 2);
        assert_eq!(chunks, vec!["&amp;&amp;", "&amp;"]);

        let long = "x".repeat(5000);
        let chunks = split_telegram_html(&long, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 4096);
    }

    #[test]
    fn transport_errors_lose_their_credential_bearing_url() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt.block_on(async {
            Client::new()
                .get("http://127.0.0.1:1/message?token=SECRET_TOKEN")
                .send()
                .await
                .unwrap_err()
        });
        let redacted = redact_transport_error(anyhow::Error::new(err));
        assert!(!format!("{redacted:#}").contains("SECRET_TOKEN"));
    }
}
