use crate::models::{ItemType, NotificationItem};
use crate::trackers::{TrackerContext, TrackerError, sel, text_clean};
use scraper::Html;

pub async fn fetch_items(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let inbox_url = ctx.join_url("messages.php?action=viewmailbox&box=1&unread=yes");
    let html = ctx
        .fetch_page(&inbox_url, "messages", Some("torrents.php"))
        .await?;

    // The inbox is paginated (20 per page by default): with more unread messages than fit on
    // page 0, the older ones would never be seen while they stay unread. Walk the pager.
    let mut candidates = parse_nexusphp_inbox(&html, ctx);
    let last_page = last_inbox_page(&html).min(MAX_INBOX_PAGES - 1);
    for page in 1..=last_page {
        let page_url = format!("{inbox_url}&page={page}");
        let page_html = ctx
            .fetch_page(&page_url, "messages", Some("torrents.php"))
            .await?;
        for item in parse_nexusphp_inbox(&page_html, ctx) {
            if !candidates.iter().any(|c| c.id == item.id) {
                candidates.push(item);
            }
        }
    }
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

/// Upper bound on inbox pages read per check (page 0 included), to keep requests polite.
const MAX_INBOX_PAGES: u32 = 10;

/// Highest `page=` (0-based) linked from the unread-mailbox pager, or 0 without a pager.
fn last_inbox_page(html: &str) -> u32 {
    let doc = Html::parse_document(html);
    doc.select(&sel("a[href]"))
        .filter_map(|a| a.value().attr("href"))
        .filter(|h| h.contains("action=viewmailbox") && h.contains("unread=yes"))
        .filter_map(|h| {
            h.split(['?', '&'])
                .find_map(|kv| kv.strip_prefix("page="))
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0)
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

#[cfg(test)]
mod pager_tests {
    use super::last_inbox_page;

    #[test]
    fn reads_the_highest_unread_inbox_page() {
        let html = r#"<p>
            <a href="?action=viewmailbox&amp;box=1&amp;unread=yes&amp;page=1">2</a>
            <a href="messages.php?action=viewmailbox&amp;box=1&amp;unread=yes&amp;page=3">4</a>
            <a href="messages.php?action=viewmailbox&amp;box=2&amp;page=9">sent box, other filter</a>
            <a href="torrents.php?page=50">torrents</a>
        </p>"#;
        assert_eq!(last_inbox_page(html), 3);
        assert_eq!(last_inbox_page("<p>no pager</p>"), 0);
    }
}
