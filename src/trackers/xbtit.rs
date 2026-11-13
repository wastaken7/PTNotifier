use crate::models::{ItemType, NotificationItem};
use crate::trackers::{TrackerContext, TrackerError, sel, text_clean};
use regex::Regex;
use scraper::Html;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub enum XbtitVariant {
    HdSpace,
    HdTorrents,
    SportsCult,
}

pub async fn fetch_items(
    ctx: &mut TrackerContext,
    variant: XbtitVariant,
) -> Result<Vec<NotificationItem>, TrackerError> {
    if ctx.state.notifications_url.is_none() {
        let home = ctx.fetch_page(&ctx.config.base_url, "user ID", None).await?;
        let doc = Html::parse_document(&home);
        static RE_UID: OnceLock<Regex> = OnceLock::new();
        let re_uid = RE_UID.get_or_init(|| Regex::new(r"uid=(\d+)").unwrap());

        for a in doc.select(&sel("a[href]")) {
            let Some(href) = a.value().attr("href") else {
                continue;
            };
            let matched = match variant {
                XbtitVariant::HdTorrents => href.contains("usercp.php?uid="),
                XbtitVariant::HdSpace | XbtitVariant::SportsCult => href.contains("page=usercp&uid="),
            };
            if matched && let Some(caps) = re_uid.captures(href) {
                let uid = &caps[1];
                let path = match variant {
                    XbtitVariant::HdTorrents => format!("usercp.php?uid={uid}&do=pm&action=list"),
                    XbtitVariant::HdSpace | XbtitVariant::SportsCult => {
                        format!("index.php?page=usercp&uid={uid}&do=pm&action=list&what=inbox")
                    }
                };
                ctx.state.notifications_url = Some(ctx.join_url(&path));
                break;
            }
        }
    }

    let Some(target_url) = ctx.state.notifications_url.clone() else {
        return Err(TrackerError::CookieExpired(
            "Initialization failed (UID not found)".into(),
        ));
    };

    let success_kw = match variant {
        XbtitVariant::HdSpace => "Favourites",
        XbtitVariant::HdTorrents => "wishlist.php",
        XbtitVariant::SportsCult => "Mailbox",
    };

    let html = ctx
        .fetch_page(&target_url, "messages", Some(success_kw))
        .await?;

    let candidates: Vec<NotificationItem> = {
        let doc = Html::parse_document(&html);
        let mut out = Vec::new();
        if let Some(form) = doc.select(&sel(r#"form[name="deleteall"]"#)).next()
            && let Some(table) = form.select(&sel("table.lista")).next()
        {
            for row in table.select(&sel("tr")).skip(1) {
                let cols: Vec<_> = row.select(&sel("td.lista")).collect();
                if cols.len() < 4 {
                    continue;
                }
                let Some(subject_link) = cols[3].select(&sel("a[href]")).next() else {
                    continue;
                };
                let subject = text_clean(subject_link);
                let href = subject_link.value().attr("href").unwrap_or_default();
                let link = ctx.join_url(href);
                let item_id = if link.contains("id=") {
                    link.split("id=")
                        .last()
                        .unwrap_or(&link)
                        .split('&')
                        .next()
                        .unwrap_or(&link)
                        .to_string()
                } else {
                    link.clone()
                };
                if ctx.is_processed(&item_id) {
                    continue;
                }
                let sender = text_clean(cols[1]);
                let date = text_clean(cols[2]).replace('\u{00a0}', " ");

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

    let mut items = Vec::with_capacity(candidates.len());
    for mut item in candidates {
        if let Ok(body_html) = ctx.fetch_page(&item.url, "message body", None).await {
            let doc = Html::parse_document(&body_html);
            for header in doc.select(&sel("td.header")) {
                if text_clean(header).contains("Subject:") {
                    if let Some(parent_row) = header.parent().and_then(scraper::ElementRef::wrap) {
                        let mut sib = parent_row.next_sibling();
                        while let Some(node) = sib {
                            if let Some(next_tr) = scraper::ElementRef::wrap(node)
                                && next_tr.value().name() == "tr"
                            {
                                let body_sel = match variant {
                                    XbtitVariant::HdTorrents => "td",
                                    _ => "td.lista",
                                };
                                if let Some(td) = next_tr.select(&sel(body_sel)).next() {
                                    item.body = Some(td.html());
                                }
                                break;
                            }
                            sib = node.next_sibling();
                        }
                    }
                    break;
                }
            }
        }
        items.push(item);
    }

    Ok(items)
}
