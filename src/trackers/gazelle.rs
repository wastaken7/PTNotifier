use crate::models::{ItemType, NotificationItem};
use crate::trackers::{TrackerContext, TrackerError, net_error, sel, text_clean};
use regex::Regex;
use scraper::Html;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

pub async fn fetch_bjshare_or_brasil(
    ctx: &mut TrackerContext,
    is_bjshare: bool,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("inbox.php?sort=unread");
    let staff_url = ctx.join_url("staffpm.php");

    let mut items = parse_gazelle_table(ctx, &inbox_url, false, is_bjshare).await?;
    items.extend(parse_gazelle_table(ctx, &staff_url, true, is_bjshare).await?);
    Ok(items)
}

async fn parse_gazelle_table(
    ctx: &TrackerContext,
    url: &str,
    is_staff: bool,
    is_bjshare: bool,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let label = if is_staff { "staff messages" } else { "messages" };
    let html = ctx.fetch_page(url, label, Some("forums.php")).await?;
    let conv_links: Vec<(String, String)> = {
        let doc = Html::parse_document(&html);
        if !is_bjshare && doc.select(&sel("#messageform")).next().is_none() {
            return Ok(Vec::new());
        }
        let mut links = Vec::new();
        for row in doc.select(&sel("table.message_table tr.unreadpm")) {
            let cols: Vec<_> = row.select(&sel("td")).collect();
            if cols.len() < 3 {
                continue;
            }
            if let Some(a) = cols[1].select(&sel("a[href]")).next() {
                let subject = text_clean(a);
                let href = a.value().attr("href").unwrap_or_default();
                links.push((ctx.join_url(href), subject));
            }
        }
        links
    };

    let mut items = Vec::new();
    for (link, subject) in conv_links {
        let body_html = ctx.fetch_page(&link, "message body", None).await?;
        let doc = Html::parse_document(&body_html);
        for box_el in doc.select(&sel("div.box.vertical_space")) {
            let body_div = box_el.select(&sel("div.body")).next();
            let raw_id = if is_bjshare && is_staff {
                box_el.value().attr("id").map(|s| s.replace("post", ""))
            } else {
                body_div
                    .and_then(|b| b.value().attr("id"))
                    .map(|s| s.replace("message", ""))
            };

            let Some(message_id) = raw_id.filter(|s| !s.is_empty()) else {
                continue;
            };
            let item_id = qualified_id(&message_id, is_staff);
            if ctx.is_processed(&item_id) {
                continue;
            }

            let mut sender = if is_bjshare && is_staff {
                "Staff System".to_string()
            } else {
                "System".to_string()
            };
            let mut date = "Unknown".to_string();

            if let Some(head) = box_el.select(&sel("div.head")).next() {
                if let Some(a) = head.select(&sel("a[href]")).next() {
                    let s = text_clean(a);
                    if !s.is_empty() && s != "Citar" {
                        sender = s;
                    }
                }
                if let Some(t) = head.select(&sel("span.time")).next() {
                    date = text_clean(t);
                }
            }

            let body = body_div.map(|b| b.html());
            let anchor = if is_bjshare && is_staff {
                format!("{link}#post{message_id}")
            } else {
                format!("{link}#message{message_id}")
            };

            items.push(NotificationItem {
                item_type: ItemType::Message,
                id: item_id,
                title: None,
                subject: Some(subject.clone()),
                sender: Some(sender),
                body,
                date,
                url: anchor,
                is_staff,
                favicon: None,
                mark_read_url: None,
            });
        }
    }
    Ok(items)
}

pub async fn fetch_gpw(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("inbox.php?sort=unread");
    let staff_url = ctx.join_url("staffpm.php");
    let mut items = parse_gpw_table(ctx, &inbox_url, false).await?;
    items.extend(parse_gpw_table(ctx, &staff_url, true).await?);
    Ok(items)
}

async fn parse_gpw_table(
    ctx: &TrackerContext,
    url: &str,
    is_staff: bool,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let label = if is_staff { "staff messages" } else { "messages" };
    let html = ctx.fetch_page(url, label, Some("forums.php")).await?;

    let conv_links: Vec<(String, String)> = {
        let doc = Html::parse_document(&html);
        let mut links = Vec::new();
        for table in doc.select(&sel("table.TableUserInbox, table.Table")) {
            for row in table.select(&sel("tr.Table-row")) {
                let cols: Vec<_> = row.select(&sel("td")).collect();
                if cols.len() < 3 {
                    continue;
                }
                if cols[1].select(&sel("strong")).next().is_none() {
                    continue;
                }
                if let Some(a) = cols[1].select(&sel("a[href]")).next() {
                    let subject = text_clean(a);
                    let href = a.value().attr("href").unwrap_or_default();
                    links.push((ctx.join_url(href), subject));
                }
            }
        }
        links
    };

    let mut items = Vec::new();
    for (link, subject) in conv_links {
        let body_html = ctx.fetch_page(&link, "message body", None).await?;
        let doc = Html::parse_document(&body_html);
        for box_el in doc.select(&sel("div.Box")) {
            let Some(body_div) = box_el
                .select(&sel("div[id^=\"message\"]"))
                .next()
            else {
                continue;
            };
            let message_id = body_div
                .value()
                .attr("id")
                .unwrap_or_default()
                .replace("message", "");
            let item_id = qualified_id(&message_id, is_staff);
            if message_id.is_empty() || ctx.is_processed(&item_id) {
                continue;
            }

            let mut sender = "System".to_string();
            let mut date = "Unknown".to_string();
            if let Some(head) = box_el.select(&sel("div.Box-header")).next() {
                if let Some(a) = head.select(&sel(r#"a[href*="user.php?id="]"#)).next() {
                    sender = text_clean(a);
                }
                if let Some(d) = head.select(&sel("span.tooltipstered")).next() {
                    date = text_clean(d);
                }
            }

            items.push(NotificationItem {
                item_type: ItemType::Message,
                id: item_id,
                title: None,
                subject: Some(subject.clone()),
                sender: Some(sender),
                body: Some(body_div.html()),
                date,
                url: format!("{link}#message{message_id}"),
                is_staff,
                favicon: None,
                mark_read_url: None,
            });
        }
    }
    Ok(items)
}

pub async fn fetch_orpheus(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    if ctx.config.api_token.trim().is_empty() {
        return Err(TrackerError::Config("Orpheus API token is required".into()));
    }
    let url = ctx.join_url("ajax.php?action=inbox");
    ctx.respect_rate_limit().await;
    let resp = ctx
        .client
        .get(&url)
        .header("Accept", "application/json")
        .header(
            "Authorization",
            format!("token {}", ctx.config.api_token.trim()),
        )
        .send()
        .await
        .map_err(|e| net_error("Orpheus inbox", e))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| net_error("Orpheus read", e))?;
    if !status.is_success() {
        return Err(TrackerError::CookieExpired(format!(
            "Orpheus HTTP {status}"
        )));
    }

    let data: Value = serde_json::from_str(&text)
        .map_err(|e| TrackerError::Parse(format!("Orpheus JSON: {e}")))?;
    if data.get("status").and_then(|v| v.as_str()) != Some("success") {
        return Err(TrackerError::CookieExpired(
            "Orpheus API returned non-success".into(),
        ));
    }

    let messages = data
        .pointer("/response/messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::new();
    for msg in messages {
        let conv_id = msg
            .get("convId")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default();
        if conv_id.is_empty() {
            continue;
        }
        // A conversation keeps its ID across replies, so key each update by the conversation
        // plus the date of its latest message: a new reply yields a new item.
        let last_date = msg.get("date").and_then(|v| v.as_str()).unwrap_or_default();
        let item_id = format!("{conv_id}@{last_date}");
        if ctx.is_processed(&item_id) {
            continue;
        }
        // Our own replies also bump the date; only conversations with something unread for
        // us are new (older API responses without the flag are treated as unread).
        if msg.get("unread").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }

        let conv_url = ctx.join_url(&format!("ajax.php?action=inbox&type=viewconv&id={conv_id}"));
        ctx.respect_rate_limit().await;
        let body = match ctx
            .client
            .get(&conv_url)
            .header("Accept", "application/json")
            .header("Authorization", format!("token {}", ctx.config.api_token.trim()))
            .send()
            .await
        {
            Ok(r) => r
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| {
                    v.pointer("/response/messages")
                        .and_then(|m| m.as_array())
                        .and_then(|arr| arr.last())
                        .and_then(|last| last.get("body"))
                        .and_then(|b| b.as_str())
                        .map(str::to_string)
                }),
            Err(_) => None,
        };

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id,
            title: None,
            subject: Some(
                msg.get("subject")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No Subject")
                    .to_string(),
            ),
            sender: Some(
                msg.get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("System")
                    .to_string(),
            ),
            body,
            date: msg
                .get("date")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            url: ctx.join_url(&format!("inbox.php?action=viewconv&id={conv_id}")),
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }

    Ok(items)
}

pub async fn fetch_anthelion(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let key = ctx.config.api_token.trim();
    if key.is_empty() {
        return Err(TrackerError::Config("Anthelion API key is required".into()));
    }
    let url = ctx.join_url(&format!("api.php?action=inbox&api_key={key}"));
    let raw = ctx.fetch_page(&url, "Anthelion inbox", None).await?;
    let data: Value =
        serde_json::from_str(&raw).map_err(|e| TrackerError::Parse(format!("Anthelion JSON: {e}")))?;

    if data.get("status").and_then(|v| v.as_str()) != Some("success") {
        return Err(TrackerError::CookieExpired("Anthelion API returned non-success".into()));
    }

    static RE_TORRENT: OnceLock<Regex> = OnceLock::new();
    let re_torrent = RE_TORRENT
        .get_or_init(|| Regex::new(r"\[torrent\]([^\[]+)\[/torrent\]").unwrap());

    let messages = data
        .pointer("/response/messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::new();
    for msg in messages {
        let message_id = msg
            .get("message_id")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default();
        if message_id.is_empty() || ctx.is_processed(&message_id) {
            continue;
        }
        let conv_id = msg
            .get("conv_id")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default();
        let raw_body = msg.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let body = re_torrent
            .replace_all(raw_body, "https://anthelion.me/torrents.php?id=$1")
            .to_string();

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: message_id,
            title: None,
            subject: Some(
                msg.get("subject")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No Subject")
                    .to_string(),
            ),
            sender: Some(
                msg.get("sender")
                    .and_then(|v| v.as_str())
                    .unwrap_or("System")
                    .to_string(),
            ),
            body: Some(body),
            date: msg
                .get("sent_date")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            url: ctx.join_url(&format!("inbox.php?action=viewconv&id={conv_id}")),
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }

    Ok(items)
}

/// GGn allows at most five API requests every ten seconds, so its calls are spaced at least
/// this far apart even when the global request delay is set lower.
const GGN_MIN_REQUEST_GAP: Duration = Duration::from_secs(2);

/// One GET against the GGn JSON API (`api.php?request=...`), authenticated with the
/// `X-API-Key` header. Returns the `response` value of a `"status": "success"` reply.
async fn ggn_get(ctx: &TrackerContext, url: &str, context: &str) -> Result<Value, TrackerError> {
    ggn_request(ctx, url, context, None).await
}

/// A GGn API call; with `form`, it is sent as a POST with that url-encoded body.
async fn ggn_request(
    ctx: &TrackerContext,
    url: &str,
    context: &str,
    form: Option<&[(&str, &str)]>,
) -> Result<Value, TrackerError> {
    ctx.check_outbound(url, context)?;
    ctx.respect_rate_limit().await;
    {
        // Only waits when the global request delay is shorter than the GGn floor.
        static LAST: OnceLock<tokio::sync::Mutex<Option<Instant>>> = OnceLock::new();
        let mut last = LAST.get_or_init(|| tokio::sync::Mutex::new(None)).lock().await;
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < GGN_MIN_REQUEST_GAP {
                tokio::time::sleep(GGN_MIN_REQUEST_GAP - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }
    let req = match form {
        Some(fields) => ctx.client.post(url).form(fields),
        None => ctx.client.get(url),
    };
    let resp = req
        .header("Accept", "application/json")
        .header("X-API-Key", ctx.config.api_token.trim())
        .send()
        .await
        .map_err(|e| net_error(context, e))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| net_error(&format!("{context} body read"), e))?;
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(TrackerError::CookieExpired(format!(
            "{context}: HTTP {status} (invalid API key or missing permission)"
        )));
    }
    if !status.is_success() {
        return Err(TrackerError::Network(format!("{context}: HTTP {status}")));
    }
    let data: Value = serde_json::from_str(&text)
        .map_err(|e| TrackerError::Parse(format!("{context} JSON: {e}")))?;
    if data.get("status").and_then(|v| v.as_str()) != Some("success") {
        let detail = data
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("non-success status");
        return Err(TrackerError::CookieExpired(format!("{context}: {detail}")));
    }
    Ok(data.get("response").cloned().unwrap_or(Value::Null))
}

/// A JSON scalar as a string: GGn mixes numeric and string IDs between endpoints.
fn json_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

pub async fn fetch_ggn(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    if ctx.config.api_token.trim().is_empty() {
        return Err(TrackerError::Config("GazelleGames API key is required".into()));
    }

    let inbox_url = ctx.join_url("api.php?request=inbox&sort=unread");
    let inbox = ggn_get(ctx, &inbox_url, "GGn inbox").await?;
    let conversations = inbox
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::new();
    for conv in conversations {
        // Our own replies also bump the date; only conversations unread for us are new.
        // (Not a `break`: sticky conversations may be listed ahead of unread ones.)
        if conv.get("unread").and_then(|v| v.as_bool()) != Some(true) {
            continue;
        }
        let conv_id = json_str(conv.get("convId"));
        if conv_id.is_empty() {
            continue;
        }
        // A conversation keeps its ID across replies, so each update is keyed by the
        // conversation plus the date of its latest message.
        let date = json_str(conv.get("date"));
        let item_id = format!("{conv_id}@{date}");
        if ctx.is_processed(&item_id) {
            continue;
        }

        let conv_url = ctx.join_url(&format!("api.php?request=inbox&type=viewconv&id={conv_id}"));
        let last_msg = match ggn_get(ctx, &conv_url, "GGn conversation").await {
            Ok(v) => v
                .get("messages")
                .and_then(|m| m.as_array())
                .and_then(|arr| arr.last())
                .cloned(),
            Err(e) => {
                warn!(tracker = %ctx.config.name, "{e}");
                None
            }
        };
        let body = last_msg
            .as_ref()
            .and_then(|m| m.get("body"))
            .and_then(|b| b.as_str())
            .map(str::to_string);
        let sender = last_msg
            .as_ref()
            .map(|m| json_str(m.get("senderName")))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "System".to_string());

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id,
            title: None,
            // GGn sends subjects HTML-escaped (`&#39;`).
            subject: Some(crate::notify::decode_html_entities(
                conv.get("subject")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("No Subject"),
            )),
            sender: Some(sender),
            body,
            date,
            url: ctx.join_url(&format!("inbox.php?action=viewconv&id={conv_id}")),
            is_staff: false,
            favicon: None,
            // Carries the conversation ID for ggn_mark_read, which POSTs it to the API.
            mark_read_url: Some(conv_id),
        });
    }

    // Deleted-torrent notifications need the key's Torrents permission, which is optional:
    // a key without it still delivers PMs, so a failure here is only logged.
    let notifs_url = ctx.join_url("api.php?request=delete_notifs&limit=50");
    match ggn_get(ctx, &notifs_url, "GGn deleted torrent notifications").await {
        Ok(list) => {
            for n in list.as_array().cloned().unwrap_or_default() {
                if !matches!(json_str(n.get("UnRead")).as_str(), "1" | "true") {
                    continue;
                }
                let torrent_id = json_str(n.get("TorrentID"));
                let delete_time = json_str(n.get("DeleteTime"));
                if torrent_id.is_empty() {
                    continue;
                }
                let item_id = format!("deleted_{torrent_id}@{delete_time}");
                if ctx.is_processed(&item_id) {
                    continue;
                }
                let name = json_str(n.get("TorrentName"));
                let reason = json_str(n.get("DeleteReason"));
                let note = json_str(n.get("Note"));
                let mut body = format!("Reason: {}", if reason.is_empty() { "-" } else { &reason });
                if !note.is_empty() {
                    body.push_str(&format!("\nNote: {note}"));
                }
                let replacements: Vec<String> = n
                    .get("ReplacementTorrents")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().map(|x| json_str(Some(x))).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default();
                for r in &replacements {
                    body.push_str(&format!(
                        "\nReplacement: {}",
                        ctx.join_url(&format!("torrents.php?torrentid={r}"))
                    ));
                }
                let group_id = json_str(n.get("GroupID"));
                let url = if group_id.is_empty() {
                    ctx.join_url("")
                } else {
                    ctx.join_url(&format!("torrents.php?id={group_id}"))
                };
                items.push(NotificationItem {
                    item_type: ItemType::Notification,
                    id: item_id,
                    title: Some(format!(
                        "Torrent deleted: {}",
                        if name.is_empty() { torrent_id.clone() } else { crate::notify::decode_html_entities(&name) }
                    )),
                    subject: None,
                    sender: None,
                    body: Some(body),
                    date: delete_time,
                    url,
                    is_staff: false,
                    favicon: None,
                    mark_read_url: None,
                });
            }
        }
        // A key without the Torrents permission gets HTTP 403 "out of scope of API key" on
        // every check; that is a setup choice, not a fault worth a warning each poll.
        Err(TrackerError::CookieExpired(m)) if m.contains("HTTP 403") => {
            debug!(tracker = %ctx.config.name, "{m}");
        }
        Err(e) => warn!(tracker = %ctx.config.name, "{e}"),
    }

    Ok(items)
}

/// Marks a conversation read. GGn only reads `messages[]` from a POST body: the same field
/// in the query string is rejected with "No conversations specified".
pub async fn ggn_mark_read(ctx: &TrackerContext, conv_id: &str) {
    let url = ctx.join_url("api.php?request=inbox&type=markread");
    let form = [("messages[]", conv_id)];
    if let Err(e) = ggn_request(ctx, &url, "GGn mark read", Some(&form)).await {
        warn!(tracker = %ctx.config.name, "Failed to mark GGn conversation as read: {e}");
    }
}

/// Inbox and staff-inbox messages are numbered separately on some Gazelle installs, so a
/// staff ID gets a prefix to keep it from colliding with a normal message in the shared
/// processed set. Normal IDs stay bare so existing state remains valid.
fn qualified_id(message_id: &str, is_staff: bool) -> String {
    if is_staff {
        format!("staff_{message_id}")
    } else {
        message_id.to_string()
    }
}

#[cfg(test)]
mod qualified_id_tests {
    use super::qualified_id;

    #[test]
    fn staff_ids_do_not_collide_with_inbox_ids() {
        assert_eq!(qualified_id("42", false), "42");
        assert_eq!(qualified_id("42", true), "staff_42");
        assert_ne!(qualified_id("42", false), qualified_id("42", true));
    }
}
