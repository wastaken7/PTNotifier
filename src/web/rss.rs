use crate::models::NotificationHistoryEntry;
use crate::notify::format_markdown;
use chrono::{DateTime, Utc};

/// Builds an RSS 2.0 XML document from notification history entries.
pub fn build_rss_feed(
    entries: &[NotificationHistoryEntry],
    base_url: &str,
    self_url: &str,
    filter_label: Option<&str>,
) -> String {
    let channel_title = match filter_label {
        Some(label) if !label.is_empty() => format!("PTNotifier — {label}"),
        _ => String::from("PTNotifier"),
    };

    let last_build_date = entries
        .first()
        .map(|e| format_rfc2822(&e.created_at))
        .unwrap_or_else(|| Utc::now().to_rfc2822());

    let mut xml = String::with_capacity(2048);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n");
    xml.push_str("  <channel>\n");
    xml.push_str(&format!("    <title>{}</title>\n", escape_xml(&channel_title)));
    xml.push_str(&format!("    <link>{}</link>\n", escape_xml(base_url)));
    xml.push_str(
        "    <description>Private tracker notifications, private messages, and error alerts</description>\n",
    );
    xml.push_str(&format!(
        "    <atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\" />\n",
        escape_xml(self_url)
    ));
    xml.push_str(&format!(
        "    <lastBuildDate>{}</lastBuildDate>\n",
        escape_xml(&last_build_date)
    ));

    for entry in entries {
        let title = item_title(entry);
        let link = if entry.url.trim().is_empty() {
            base_url
        } else {
            entry.url.trim()
        };
        let guid = format!(
            "ptnotifier:{}:{}:{}",
            entry.id, entry.tracker_id, entry.external_id
        );
        let pub_date = format_rfc2822(&entry.created_at);
        let desc = item_description(entry);

        xml.push_str("    <item>\n");
        xml.push_str(&format!("      <title>{}</title>\n", escape_xml(&title)));
        xml.push_str(&format!("      <link>{}</link>\n", escape_xml(link)));
        xml.push_str(&format!(
            "      <guid isPermaLink=\"false\">{}</guid>\n",
            escape_xml(&guid)
        ));
        xml.push_str(&format!(
            "      <pubDate>{}</pubDate>\n",
            escape_xml(&pub_date)
        ));
        xml.push_str(&format!(
            "      <category>{}</category>\n",
            escape_xml(&entry.item_type)
        ));
        xml.push_str(&format!(
            "      <category>{}</category>\n",
            escape_xml(&entry.tracker_name)
        ));
        xml.push_str(&format!(
            "      <description>{}</description>\n",
            escape_xml(&desc)
        ));
        xml.push_str("    </item>\n");
    }

    xml.push_str("  </channel>\n");
    xml.push_str("</rss>\n");
    xml
}

fn item_title(entry: &NotificationHistoryEntry) -> String {
    match entry.item_type.as_str() {
        "message" => {
            let subject = entry
                .subject
                .as_deref()
                .or(entry.title.as_deref())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Private Message");
            let staff = if entry.is_staff { " [Staff]" } else { "" };
            match entry.sender.as_deref().filter(|s| !s.trim().is_empty()) {
                Some(sender) => format!("[{}] PM from {}{}: {}", entry.tracker_name, sender, staff, subject),
                None => format!("[{}] PM{}: {}", entry.tracker_name, staff, subject),
            }
        }
        "error" => {
            let summary = entry
                .title
                .as_deref()
                .or(entry.subject.as_deref())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Tracker Error");
            format!("[{}] Error: {}", entry.tracker_name, summary)
        }
        _ => {
            let summary = entry
                .title
                .as_deref()
                .or(entry.subject.as_deref())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Notification");
            format!("[{}] {}", entry.tracker_name, summary)
        }
    }
}

fn item_description(entry: &NotificationHistoryEntry) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Tracker: {} ({})", entry.tracker_name, entry.item_type));

    if let Some(sender) = entry.sender.as_deref().filter(|s| !s.trim().is_empty()) {
        let staff = if entry.is_staff { " [Staff]" } else { "" };
        parts.push(format!("From: {sender}{staff}"));
    }

    if let Some(subject) = entry.subject.as_deref().filter(|s| !s.trim().is_empty()) {
        if entry.title.as_deref() != Some(subject) {
            parts.push(format!("Subject: {subject}"));
        }
    }

    if !entry.item_date.trim().is_empty() {
        parts.push(format!("Date: {}", entry.item_date.trim()));
    }

    if let Some(body) = entry.body.as_deref().filter(|s| !s.trim().is_empty()) {
        let formatted = format_markdown(body);
        if !formatted.is_empty() {
            parts.push(String::new());
            parts.push(formatted);
        }
    }

    parts.join("\n")
}

fn format_rfc2822(rfc3339: &str) -> String {
    DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| dt.to_rfc2822())
        .unwrap_or_else(|_| Utc::now().to_rfc2822())
}

fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Strip invalid XML 1.0 control characters
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(item_type: &str) -> NotificationHistoryEntry {
        NotificationHistoryEntry {
            id: 42,
            tracker_id: "tr-1".into(),
            tracker_name: "TestTracker".into(),
            item_type: item_type.into(),
            external_id: "ext-99".into(),
            title: Some("New alert & <update>".into()),
            subject: Some("Important PM".into()),
            sender: Some("Admin".into()),
            body: Some("<b>Hello</b> world\x00!".into()),
            is_staff: true,
            item_date: "2026-11-13 18:00:00".into(),
            url: "https://tracker.example/inbox?id=99&view=1".into(),
            created_at: "2026-11-13T21:00:00Z".into(),
        }
    }

    #[test]
    fn builds_valid_empty_rss_feed() {
        let xml = build_rss_feed(&[], "http://localhost:8085", "http://localhost:8085/rss", None);
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<rss version=\"2.0\""));
        assert!(xml.contains("<title>PTNotifier</title>"));
        assert!(xml.contains("<atom:link href=\"http://localhost:8085/rss\" rel=\"self\""));
        assert!(!xml.contains("<item>"));
    }

    #[test]
    fn formats_pm_notification_and_error_entries_with_xml_escaping() {
        let pm = sample_entry("message");
        let mut notif = sample_entry("notification");
        notif.id = 43;
        notif.sender = None;
        notif.is_staff = false;

        let mut err = sample_entry("error");
        err.id = 44;
        err.title = Some("TestTracker Check Failed".into());
        err.body = Some("Cookie expired".into());

        let xml = build_rss_feed(
            &[pm, notif, err],
            "http://localhost:8085",
            "http://localhost:8085/rss",
            Some("message"),
        );

        assert!(xml.contains("<title>PTNotifier — message</title>"));
        assert!(xml.contains("[TestTracker] PM from Admin [Staff]: Important PM"));
        assert!(xml.contains("[TestTracker] New alert &amp; &lt;update&gt;"));
        assert!(xml.contains("[TestTracker] Error: TestTracker Check Failed"));
        assert!(xml.contains("<link>https://tracker.example/inbox?id=99&amp;view=1</link>"));
        assert!(xml.contains("<guid isPermaLink=\"false\">ptnotifier:42:tr-1:ext-99</guid>"));
        assert!(xml.contains("Fri, 13 Nov 2026 21:00:00 +0000"));
        assert!(!xml.contains('\x00'));
        assert!(xml.contains("**Hello** world!"));
    }
}
