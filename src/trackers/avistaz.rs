use crate::models::{ItemType, NotificationItem};
use crate::trackers::{TrackerContext, TrackerError, sel, text_clean};
use scraper::Html;

fn avistaz_favicon(base_url: &str) -> &'static str {
    let lower = base_url.to_lowercase();
    if lower.contains("privatehd") {
        "https://privatehd.to/images/privatehd-favicon.png"
    } else if lower.contains("cinemaz") {
        "https://cinemaz.to/images/cinemaz-favicon.png"
    } else if lower.contains("exoticaz") {
        "https://i.exoticaz.to/images/favicon.png"
    } else {
        "https://avistaz.to/images/avistaz-favicon.png"
    }
}

pub async fn fetch_items(ctx: &mut TrackerContext) -> Result<Vec<NotificationItem>, TrackerError> {
    let mut items = Vec::new();
    let favicon = avistaz_favicon(&ctx.config.base_url).to_string();

    let notif_url = ctx.join_url("notifications");
    let notif_html = ctx.fetch_page(&notif_url, "notifications", None).await?;
    items.extend(parse_notifications(&notif_html, &notif_url, &favicon, ctx));

    let msg_url = ctx.join_url("messenger");
    let msg_html = ctx
        .fetch_page(&msg_url, "messages", Some("messenger/new"))
        .await?;
    let mut msgs = parse_messages(&msg_html, &favicon, ctx);
    for m in &mut msgs {
        if let Ok(body_html) = ctx.fetch_page(&m.url, "message body", None).await {
            let doc = Html::parse_document(&body_html);
            if let Some(last) = doc.select(&sel("div.message-content, div.panel-body")).last() {
                m.body = Some(last.html());
            }
        }
    }
    items.extend(msgs);

    Ok(items)
}

fn parse_notifications(
    html: &str,
    notif_url: &str,
    favicon: &str,
    ctx: &TrackerContext,
) -> Vec<NotificationItem> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();

    for row in doc.select(&sel("table.table-hover tbody tr")) {
        let cols: Vec<_> = row.select(&sel("td")).collect();
        if cols.len() < 3 {
            continue;
        }
        let date = text_clean(cols[0]);
        let title_link = cols[1].select(&sel("a[href]")).next();
        let title = title_link.map(text_clean).unwrap_or_else(|| text_clean(cols[1]));
        let href = title_link
            .and_then(|a| a.value().attr("href"))
            .map(|h| ctx.join_url(h));
        let subject = text_clean(cols[2]);
        // Linked rows are keyed by their destination. Linkless rows would all share the
        // notifications-page URL, so key them by their (stable) content instead; the date
        // cell is left out because it may be rendered relative to now.
        let id = match &href {
            Some(link) => link.clone(),
            None => format!(
                "{notif_url}#{}",
                &crate::security::sha256_hex(&format!("{title}
{subject}"))[..16]
            ),
        };
        let link = href.unwrap_or_else(|| notif_url.to_string());

        if ctx.is_processed(&id) {
            continue;
        }

        items.push(NotificationItem {
            item_type: ItemType::Notification,
            id,
            title: Some(title),
            subject: Some(subject),
            sender: None,
            body: None,
            date,
            url: link,
            is_staff: false,
            favicon: Some(favicon.to_string()),
            mark_read_url: None,
        });
    }
    items
}

fn parse_messages(html: &str, favicon: &str, ctx: &TrackerContext) -> Vec<NotificationItem> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();

    for row in doc.select(&sel("table.table-hover tbody tr.info.text-bold")) {
        let cols: Vec<_> = row.select(&sel("td")).collect();
        if cols.len() < 5 {
            continue;
        }
        let sender = text_clean(cols[2]);
        let Some(subject_link) = cols[3].select(&sel("a[href]")).next() else {
            continue;
        };
        let subject = text_clean(subject_link);
        let href = subject_link.value().attr("href").unwrap_or_default();
        let abs_url = ctx.join_url(href);
        let item_id = abs_url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&abs_url)
            .to_string();
        if ctx.is_processed(&item_id) {
            continue;
        }
        let date = text_clean(cols[4]);

        items.push(NotificationItem {
            item_type: ItemType::Message,
            id: item_id,
            title: None,
            subject: Some(subject),
            sender: Some(sender),
            body: None,
            date,
            url: abs_url,
            is_staff: false,
            favicon: Some(favicon.to_string()),
            mark_read_url: None,
        });
    }
    items
}
