use crate::models::{ItemType, NotificationItem};
use crate::trackers::{TrackerContext, TrackerError, sel, text_clean};
use scraper::Html;

pub async fn fetch_items(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("messages.php?action=viewmailbox&box=1&unread=yes");
    let html = ctx
        .fetch_page(&inbox_url, "messages", Some("torrents.php"))
        .await?;

    let candidates = parse_nexusphp_inbox(&html, ctx);
    let mut items = Vec::with_capacity(candidates.len());

    // Opening `viewmessage` marks the PM as read on NexusPHP, so only fetch bodies when the
    // user asked for messages to be marked as read.
    let fetch_bodies = ctx.should_mark_as_read();
    for mut item in candidates {
        if fetch_bodies
            && let Ok(body_html) = ctx.fetch_page(&item.url, "message body", None).await
        {
            item.body = extract_nexusphp_body(&body_html);
        }
        items.push(item);
    }

    Ok(items)
}

fn parse_nexusphp_inbox(html: &str, ctx: &TrackerContext) -> Vec<NotificationItem> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();

    let target_table = doc
        .select(&sel(r#"table[width="737"][cellpadding="4"]"#))
        .find(|t| {
            // Locale-independent: the inbox table is the one linking to individual messages.
            t.select(&sel("a[href]"))
                .any(|a| a.value().attr("href").is_some_and(|h| h.contains("action=viewmessage")))
        });

    let Some(table) = target_table else {
        return items;
    };

    for row in table.select(&sel("tr")) {
        if row.select(&sel("img.unreadpm")).next().is_none() {
            continue;
        }
        let cells: Vec<_> = row.select(&sel("td.rowfollow")).collect();
        if cells.len() < 4 {
            continue;
        }
        let Some(link) = cells[1]
            .select(&sel("a[href]"))
            .find(|a| a.value().attr("href").is_some_and(|h| h.contains("action=viewmessage")))
        else {
            continue;
        };

        let href = link.value().attr("href").unwrap_or_default();
        let msg_url = ctx.join_url(href);
        let item_id = msg_url.split("id=").last().unwrap_or(&msg_url).to_string();
        if ctx.is_processed(&item_id) {
            continue;
        }

        let subject = text_clean(link);
        let sender = text_clean(cells[2]);
        let date = cells[3]
            .select(&sel("span[title]"))
            .next()
            .and_then(|s| s.value().attr("title").map(str::to_string))
            .unwrap_or_else(|| text_clean(cells[3]));

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

fn extract_nexusphp_body(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    for table in doc.select(&sel(r#"table[width="737"]"#)) {
        if let Some(td) = table.select(&sel(r#"td[colspan="2"][align="left"]"#)).next() {
            return Some(td.html());
        }
    }
    None
}
