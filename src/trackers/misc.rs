use crate::models::{ItemType, NotificationItem};
use crate::trackers::{DEFAULT_USER_AGENT, TrackerContext, TrackerError, sel, text_clean};
use regex::Regex;
use scraper::Html;
use serde_json::Value;
use std::sync::OnceLock;

pub async fn fetch_asc(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let url = ctx.join_url("mensagens.php?do=n_lidas");
    let html = ctx.fetch_page(&url, "messages", Some("Reputação")).await?;
    let mut items = {
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        if let Some(table) = doc.select(&sel("table.table-inbox")).next() {
            for row in table.select(&sel("tr")) {
                let Some(item_id) = row.value().attr("id").map(str::to_string) else {
                    continue;
                };
                if ctx.is_processed(&item_id) {
                    continue;
                }
                let sender = row
                    .select(&sel("td.inbox-small-cells span.nick"))
                    .next()
                    .map(text_clean)
                    .unwrap_or_else(|| "System".into());
                let cells: Vec<_> = row.select(&sel("td.view-message")).collect();
                let subject = cells
                    .first()
                    .and_then(|c| c.select(&sel("a")).next())
                    .map(text_clean)
                    .unwrap_or_else(|| "No Subject".into());
                let date = cells
                    .get(1)
                    .map(|c| text_clean(*c))
                    .unwrap_or_else(|| "Unknown".into());
                let link = ctx.join_url(&format!("mensagens.php?action=ver_mp&id_mp={item_id}"));

                out.push(NotificationItem {
                    item_type: ItemType::Message,
                    id: item_id,
                    title: None,
                    subject: Some(subject),
                    sender: Some(sender),
                    body: None,
                    date,
                    url: link,
                    is_staff: false,
                    favicon: None,
                    mark_read_url: None,
                });
            }
        }
        out
    };

    let search_url = ctx.join_url("search.php");
    for item in &mut items {
        ctx.respect_rate_limit().await;
        if let Ok(resp) = ctx
            .client
            .post(&search_url)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("User-Agent", DEFAULT_USER_AGENT)
            .form(&[("mp", item.id.as_str()), ("type", "entrada")])
            .send()
            .await
            && let Ok(json) = resp.json::<Value>().await
            && let Some(raw_text) = json.get("text").and_then(|v| v.as_str())
        {
            let frag = Html::parse_fragment(raw_text);
            let txt: String = frag.root_element().text().collect();
            let sep = "============================================";
            let cleaned = txt.split(sep).next().unwrap_or(&txt).trim().to_string();
            item.body = Some(cleaned);
        }
    }

    Ok(items)
}

pub async fn fetch_btarg(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("messages.php?action=viewmailbox");
    let html = ctx
        .fetch_page(&inbox_url, "messages", Some("action=viewmessage"))
        .await?;
    static RE_ID: OnceLock<Regex> = OnceLock::new();
    let re_id = RE_ID.get_or_init(|| Regex::new(r"id=(\d+)").unwrap());
    let favicon = ctx.join_url("pic/btarg.ico");

    let mut items = {
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        if let Some(form) = doc.select(&sel(r#"form[name="mensajes"]"#)).next()
            && let Some(table) = form.select(&sel("table")).next()
        {
            for row in table.select(&sel("tr[onclick]")) {
                let cols: Vec<_> = row.select(&sel("td")).collect();
                if cols.len() < 5 {
                    continue;
                }
                let Some(a) = cols[1]
                    .select(&sel("a[href]"))
                    .find(|a| a.value().attr("href").is_some_and(|h| h.contains("action=viewmessage")))
                else {
                    continue;
                };
                let subject = text_clean(a);
                let link = ctx.join_url(a.value().attr("href").unwrap_or_default());
                let item_id = re_id
                    .captures(&link)
                    .map(|c| c[1].to_string())
                    .unwrap_or_else(|| link.clone());
                if ctx.is_processed(&item_id) {
                    continue;
                }
                let sender = text_clean(cols[2]);
                let date = text_clean(cols[3]);

                out.push(NotificationItem {
                    item_type: ItemType::Message,
                    id: item_id,
                    title: Some(sender.clone()),
                    subject: Some(subject),
                    sender: Some(sender),
                    body: None,
                    date,
                    url: link,
                    is_staff: false,
                    favicon: Some(favicon.clone()),
                    mark_read_url: None,
                });
            }
        }
        out
    };

    for item in &mut items {
        if let Ok(body_html) = ctx.fetch_page(&item.url, "message body", None).await {
            let doc = Html::parse_document(&body_html);
            if let Some(div) = doc.select(&sel(r#"div[style*="overflow:auto"]"#)).next() {
                item.body = Some(div.html());
            }
        }
    }

    Ok(items)
}

pub async fn fetch_digitalcore(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let url = ctx.join_url("api/v1/mailbox?index=0&limit=20&location=0");
    ctx.respect_rate_limit().await;
    let resp = ctx
        .client
        .get(&url)
        .header("Accept", "application/json, text/plain, */*")
        .send()
        .await
        .map_err(|e| TrackerError::Network(format!("DigitalCore API: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(TrackerError::CookieExpired(format!("DigitalCore HTTP {status}")));
    }
    let data: Vec<Value> = resp
        .json()
        .await
        .map_err(|e| TrackerError::Parse(format!("DigitalCore JSON: {e}")))?;

    let mut items = Vec::new();
    for msg in data {
        let item_id = msg
            .get("id")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default();
        if item_id.is_empty() || ctx.is_processed(&item_id) {
            continue;
        }
        let sender = msg
            .pointer("/user/username")
            .and_then(|v| v.as_str())
            .unwrap_or("System")
            .to_string();
        let subject = msg
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("No Subject")
            .to_string();
        let body = msg.get("body").and_then(|v| v.as_str()).map(str::to_string);
        let date = msg
            .get("added")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id.clone(),
            title: None,
            subject: Some(subject),
            sender: Some(sender),
            body,
            date,
            url: ctx.join_url(&format!("mailbox/{item_id}")),
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }
    Ok(items)
}

pub async fn fetch_fappaizuri(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let url = ctx.join_url("mailbox.php?inbox");
    let html = ctx.fetch_page(&url, "messages", Some("table_mb")).await?;
    let doc = Html::parse_document(&html);
    static RE_MSG: OnceLock<Regex> = OnceLock::new();
    let re_msg = RE_MSG.get_or_init(|| Regex::new(r"msgs\[(\d+)\]").unwrap());

    let mut items = Vec::new();
    for row in doc.select(&sel("tr")) {
        let Some(input) = row.select(&sel(r#"input[name^="msgs["]"#)).next() else {
            continue;
        };
        let Some(name) = input.value().attr("name") else {
            continue;
        };
        let Some(caps) = re_msg.captures(name) else {
            continue;
        };
        let item_id = caps[1].to_string();
        if ctx.is_processed(&item_id) {
            continue;
        }
        let cols: Vec<_> = row.select(&sel("th, td")).collect();
        if cols.len() < 5 {
            continue;
        }
        let sender = text_clean(cols[2]);
        let subject = cols[3]
            .select(&sel("a"))
            .last()
            .map(text_clean)
            .unwrap_or_else(|| "No Subject".into());
        let date = text_clean(cols[4]);
        let body = doc
            .select(&sel(&format!("tr#msg_{item_id} div#mpbox")))
            .next()
            .map(|d| d.html());

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id,
            title: Some(sender.clone()),
            subject: Some(subject),
            sender: Some(sender.clone()),
            body,
            date,
            url: url.clone(),
            is_staff: sender.eq_ignore_ascii_case("system"),
            favicon: None,
            mark_read_url: None,
        });
    }
    Ok(items)
}

pub async fn fetch_hdcity(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let url = ctx.join_url("messages");
    let html = ctx
        .fetch_page(&url, "messages", Some("messages?action=editmailboxes"))
        .await?;
    let doc = Html::parse_document(&html);
    let mut items = Vec::new();

    for row in doc.select(&sel("tr")) {
        let Some(cb) = row.select(&sel(r#"input[name="messages[]"]"#)).next() else {
            continue;
        };
        let Some(item_id) = cb.value().attr("value").map(str::to_string) else {
            continue;
        };
        if ctx.is_processed(&item_id) {
            continue;
        }
        let subject = row
            .select(&sel("a.altlink"))
            .next()
            .map(text_clean)
            .unwrap_or_else(|| "No Subject".into());
        let body = row
            .select(&sel(r#"div[style*="border:#89a 1px dashed;margin:6px;padding:6px;"]"#))
            .next()
            .map(|d| d.html());
        let cells: Vec<_> = row.select(&sel("td.rowfollow")).collect();
        let sender = cells
            .get(2)
            .map(|c| text_clean(*c))
            .unwrap_or_else(|| "System".into());
        let date = cells
            .get(3)
            .map(|c| text_clean(*c))
            .unwrap_or_else(|| "Unknown".into());

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id.clone(),
            title: Some(sender.clone()),
            subject: Some(subject),
            sender: Some(sender),
            body,
            date,
            url: ctx.join_url(&format!("messages?action=viewmessage&id={item_id}")),
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }
    Ok(items)
}

pub async fn fetch_iptorrents(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("inbox");
    let html = ctx
        .fetch_page(&inbox_url, "messages", Some("settings.php"))
        .await?;

    let mut items = {
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        if let Some(list) = doc.select(&sel("ol.list")).next() {
            for row in list.select(&sel("li.cRow")) {
                let raw_id = row.value().attr("id").unwrap_or_default();
                let item_id = raw_id.strip_prefix('c').unwrap_or(raw_id).to_string();
                if item_id.is_empty() || ctx.is_processed(&item_id) {
                    continue;
                }
                let author_span = row.select(&sel("span.t")).next();
                let is_staff = author_span
                    .is_some_and(|s| s.select(&sel("i.fa-warning")).next().is_some());
                let sender = author_span
                    .map(text_clean)
                    .unwrap_or_else(|| "System".into())
                    .replace("Staff Message", "")
                    .trim()
                    .to_string();
                let date = row
                    .select(&sel("span.elapsedDate"))
                    .next()
                    .and_then(|s| s.value().attr("title"))
                    .unwrap_or("Unknown")
                    .to_string();

                out.push(NotificationItem {
                    item_type: ItemType::Message,
                    id: item_id,
                    title: Some(sender.clone()),
                    subject: None,
                    sender: Some(sender),
                    body: None,
                    date,
                    url: inbox_url.clone(),
                    is_staff,
                    favicon: None,
                    mark_read_url: None,
                });
            }
        }
        out
    };

    let api_url = ctx.join_url("API.php");
    let origin = ctx.config.base_url.trim_end_matches('/').to_string();
    for item in &mut items {
        ctx.respect_rate_limit().await;
        if let Ok(resp) = ctx
            .client
            .post(&api_url)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", &origin)
            .header("Referer", &inbox_url)
            .form(&[("jxt", "2"), ("jxw", "c"), ("i", item.id.as_str())])
            .send()
            .await
            && let Ok(json) = resp.json::<Value>().await
            && let Some(fs) = json.get("Fs").and_then(|v| v.as_array())
        {
            for instr in fs {
                if let Some(arr) = instr.as_array()
                    && arr.first().and_then(|v| v.as_str()) == Some("DOM")
                    && arr.get(1).is_some_and(|v| v.to_string().contains(".msgBody"))
                    && let Some(cmds) = arr.get(1).and_then(|v| v.as_array())
                {
                    for cmd in cmds {
                        if let Some(c) = cmd.as_array()
                            && c.first().and_then(|v| v.as_str()) == Some("html")
                            && let Some(html_frag) = c.get(1).and_then(|v| v.as_str())
                        {
                            let frag = Html::parse_fragment(html_frag);
                            if let Some(bq) = frag.select(&sel("blockquote.body")).next() {
                                item.body = Some(bq.html());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(items)
}

pub async fn fetch_immortalseed(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("messages.php");
    let html = ctx
        .fetch_page(&inbox_url, "messages", Some("browse.php"))
        .await?;
    static RE_PMID: OnceLock<Regex> = OnceLock::new();
    let re_pmid = RE_PMID.get_or_init(|| Regex::new(r"pmid=(\d+)").unwrap());

    let mut items = {
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        if doc.select(&sel(r#"form[name="messageform"]"#)).next().is_some()
            && let Some(tbody) = doc.select(&sel("tbody#collapseobj_messages")).next()
        {
            for row in tbody.select(&sel("tr")) {
                let cols: Vec<_> = row.select(&sel("td")).collect();
                if cols.len() < 2 {
                    continue;
                }
                let Some(subj_div) = cols[1].select(&sel("div")).next() else {
                    continue;
                };
                let Some(a) = subj_div.select(&sel("a[href]")).next() else {
                    continue;
                };
                let subject = text_clean(a);
                let link = ctx.join_url(a.value().attr("href").unwrap_or_default());
                let item_id = re_pmid
                    .captures(&link)
                    .map(|c| c[1].to_string())
                    .unwrap_or_else(|| link.clone());
                if ctx.is_processed(&item_id) {
                    continue;
                }
                let date = subj_div
                    .select(&sel("span.smallfont"))
                    .next()
                    .map(text_clean)
                    .unwrap_or_else(|| "Unknown".into());
                let sender = cols[1]
                    .select(&sel("div.smalltext a"))
                    .next()
                    .map(text_clean)
                    .unwrap_or_else(|| "System".into());

                out.push(NotificationItem {
                    item_type: ItemType::Message,
                    id: item_id,
                    title: Some(sender.clone()),
                    subject: Some(subject),
                    sender: Some(sender),
                    body: None,
                    date,
                    url: link,
                    is_staff: false,
                    favicon: None,
                    mark_read_url: None,
                });
            }
        }
        out
    };

    for item in &mut items {
        if let Ok(body_html) = ctx.fetch_page(&item.url, "message body", None).await {
            let doc = Html::parse_document(&body_html);
            if let Some(hr) = doc.select(&sel(r#"hr[size="1"]"#)).next()
                && let Some(parent_td) = hr.parent().and_then(scraper::ElementRef::wrap)
            {
                item.body = Some(parent_td.html());
            }
        }
    }

    Ok(items)
}

pub async fn fetch_scenetime(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let url = ctx.join_url("inbox.php");
    let html = ctx
        .fetch_page(&url, "messages", Some("request.php"))
        .await?;
    let doc = Html::parse_document(&html);
    let mut items = Vec::new();

    for header in doc.select(&sel("div.view_mess")) {
        if header.value().attr("type") != Some("unread") {
            continue;
        }
        let Some(item_id) = header.value().attr("rel").map(str::to_string) else {
            continue;
        };
        if ctx.is_processed(&item_id) {
            continue;
        }
        let subject = header
            .select(&sel("li.status_icon"))
            .next()
            .map(text_clean)
            .unwrap_or_else(|| "No Subject".into());
        let date = header
            .select(&sel("span.elapsedDate"))
            .next()
            .and_then(|s| s.value().attr("title"))
            .unwrap_or("Unknown")
            .to_string();
        let sender = header
            .select(&sel("li.name"))
            .next()
            .map(text_clean)
            .unwrap_or_else(|| "System".into());
        let body = doc
            .select(&sel(&format!("div#messa_{item_id}")))
            .next()
            .map(|d| d.html());

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id.clone(),
            title: Some(sender.clone()),
            subject: Some(subject),
            sender: Some(sender),
            body,
            date,
            url: ctx.join_url(&format!("inbox.php?id={item_id}")),
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }
    Ok(items)
}

pub async fn fetch_torrentday(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let url = ctx.join_url("m");
    let html = ctx
        .fetch_page(&url, "messages", Some("mybonus.php"))
        .await?;

    let mut items = {
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        if let Some(table) = doc.select(&sel("table.fw.t1")).next() {
            for row in table.select(&sel("tr")).skip(1) {
                let cells: Vec<_> = row.select(&sel("td")).collect();
                if cells.len() < 3 {
                    continue;
                }
                if cells[0]
                    .select(&sel(r#"img[src*="unreadMsg.png"]"#))
                    .next()
                    .is_none()
                {
                    continue;
                }
                let sender = cells[0]
                    .select(&sel("a"))
                    .next()
                    .map(text_clean)
                    .unwrap_or_else(|| "System".into());
                let Some(subj_link) = cells[1].select(&sel("a[href]")).next() else {
                    continue;
                };
                let subject = text_clean(subj_link);
                let rel = subj_link.value().attr("href").unwrap_or_default();
                let item_id = rel
                    .rsplit('/')
                    .next()
                    .unwrap_or(rel)
                    .split('#')
                    .next()
                    .unwrap_or(rel)
                    .to_string();
                if ctx.is_processed(&item_id) {
                    continue;
                }
                let date = text_clean(cells[2]);

                out.push(NotificationItem {
                    item_type: ItemType::Message,
                    id: item_id,
                    title: Some(sender.clone()),
                    subject: Some(subject),
                    sender: Some(sender.clone()),
                    body: None,
                    date,
                    url: ctx.join_url(rel),
                    is_staff: sender.eq_ignore_ascii_case("system"),
                    favicon: None,
                    mark_read_url: None,
                });
            }
        }
        out
    };

    for item in &mut items {
        if let Ok(body_html) = ctx.fetch_page(&item.url, "message body", None).await {
            let doc = Html::parse_document(&body_html);
            if let Some(last) = doc.select(&sel("div.postContainer")).last()
                && let Some(content) = last.select(&sel("div.postContents")).next()
            {
                item.body = Some(content.html());
            }
        }
    }
    Ok(items)
}

pub async fn fetch_torrentleech(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    if ctx.state.notifications_url.is_none() {
        let home = ctx.fetch_page(&ctx.config.base_url, "user ID", None).await?;
        let doc = Html::parse_document(&home);
        static RE_PROFILE: OnceLock<Regex> = OnceLock::new();
        let re_prof = RE_PROFILE.get_or_init(|| Regex::new(r"/profile/([^/]+)/").unwrap());

        for span in doc.select(&sel("span.link[onclick]")) {
            if let Some(onclick) = span.value().attr("onclick")
                && let Some(caps) = re_prof.captures(onclick)
            {
                let username = &caps[1];
                ctx.state.notifications_url =
                    Some(ctx.join_url(&format!("profile/{username}/notifications")));
                break;
            }
        }
    }

    let Some(target_url) = ctx.state.notifications_url.clone() else {
        return Err(TrackerError::CookieExpired(
            "Initialization failed (Username not found)".into(),
        ));
    };

    let html = ctx
        .fetch_page(&target_url, "notifications", Some("snatchlist"))
        .await?;
    let doc = Html::parse_document(&html);
    let mut items = Vec::new();

    for row in doc.select(&sel("table#notificationsTable tbody tr")) {
        let cols: Vec<_> = row.select(&sel("td")).collect();
        if cols.len() < 4 {
            continue;
        }
        let Some(item_id) = cols[1].value().attr("data-sort").map(str::to_string) else {
            continue;
        };
        if ctx.is_processed(&item_id) {
            continue;
        }
        let date = text_clean(cols[1]);
        let msg_text = text_clean(cols[3]);
        let link = cols[3]
            .select(&sel("a[href]"))
            .next()
            .and_then(|a| a.value().attr("href"))
            .map(|h| ctx.join_url(h))
            .unwrap_or_else(|| target_url.clone());

        items.push(NotificationItem {
            item_type: ItemType::Notification,
            id: item_id,
            title: Some("System".into()),
            subject: Some(msg_text),
            sender: None,
            body: None,
            date,
            url: link,
            is_staff: false,
            favicon: None,
            mark_read_url: None,
        });
    }

    Ok(items)
}

pub async fn fetch_yggreborn(
    ctx: &mut TrackerContext,
) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("inbox/");
    let html = ctx
        .fetch_page(&inbox_url, "inbox", Some("Mon compte"))
        .await?;

    let mut items = {
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        for row in doc.select(&sel("tr.row-hover")) {
            let cols: Vec<_> = row.select(&sel("td")).collect();
            if cols.len() < 3 {
                continue;
            }
            if cols[0].select(&sel(r#"span[title="Non lu"]"#)).next().is_none() {
                continue;
            }
            let Some(a) = cols[2].select(&sel("a[href]")).next() else {
                continue;
            };
            let subject = text_clean(a);
            let href = a.value().attr("href").unwrap_or_default();
            let link = ctx.join_url(href);
            let msg_id = href.rsplit('/').next().unwrap_or(href).to_string();
            if msg_id.is_empty() || ctx.is_processed(&msg_id) {
                continue;
            }
            out.push(NotificationItem {
                item_type: ItemType::Message,
                id: msg_id,
                title: None,
                subject: Some(subject),
                sender: None,
                body: None,
                date: "Unknown".into(),
                url: link,
                is_staff: false,
                favicon: None,
                mark_read_url: None,
            });
        }
        out
    };

    for item in &mut items {
        if let Ok(body_html) = ctx.fetch_page(&item.url, "message body", None).await {
            let doc = Html::parse_document(&body_html);
            if let Some(meta) = doc
                .select(&sel(r#"p.text-xs[style*="var(--text-dark-light)"]"#))
                .next()
            {
                if let Some(strong) = meta.select(&sel("strong")).next() {
                    item.sender = Some(text_clean(strong));
                }
                let full = text_clean(meta);
                if let Some((_, date_part)) = full.rsplit_once('·') {
                    item.date = date_part.trim().to_string();
                }
            }
            if let Some(body) = doc.select(&sel("div.whitespace-pre-wrap")).next() {
                item.body = Some(body.html());
            }
        }
    }

    Ok(items)
}
