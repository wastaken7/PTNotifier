import re
from typing import Any

import httpx

import config
from utils.console import log


async def send_gotify(
    item: dict[str, Any],
    tracker_name: str,
    _base_url: str,
    notifications_url: str,
) -> None:
    """Sends a formatted notification to Gotify.

    Args:
        item (dict[str, Any]): The item to send.
        tracker_name (str): The name of the tracker.
        _base_url (str): The base URL of the tracker.
        notifications_url (str): The URL to the notifications page.

    Returns:
        None
    """
    gotify_url: str = config.SETTINGS.get("GOTIFY_URL", "").rstrip("/")
    gotify_token: str = config.SETTINGS.get("GOTIFY_TOKEN", "")

    if not gotify_url or not gotify_token:
        log.error("GOTIFY_URL and GOTIFY_TOKEN not set in config.py.")
        return

    icon = "🔔" if item.get("type") == "notification" else "📩"
    title = f"{tracker_name} - {icon} New {str(item.get('type', '')).capitalize()}"

    content = ""
    if item.get("is_staff"):
        content += "⚠️ **STAFF MESSAGE** ⚠️\n\n"

    if item.get("sender"):
        content += f"👤 {item['sender']}\n\n"

    if item.get("title"):
        content += f"**Title:** {item['title']}\n\n"

    if item.get("subject"):
        content += f"**Subject:** {item['subject']}\n\n"

    if item.get("body"):
        clean_body = format_for_gotify(item.get("body", ""))
        content += f"**Body:** {clean_body}\n\n"

    content += f"**Date:** {item.get('date', '')}\n\n"
    content += f"[Open Notification]({notifications_url})"

    payload: dict[str, Any] = {
        "title": title,
        "message": content,
        "priority": 5,
        "extras": {
            "client::display": {"contentType": "text/markdown"},
        },
    }

    url = f"{gotify_url}/message?token={gotify_token}"

    async with httpx.AsyncClient() as client:
        try:
            resp = await client.post(url, json=payload)
            resp.raise_for_status()
            if not resp.is_success:
                log.error(f"Gotify Error: {resp.text}")
        except Exception as e:
            log.error(f"Gotify Exception: {e}")
            log.debug("Gotify error details", exc_info=True)


def format_for_gotify(raw_description: str) -> str:
    """Converts BBCode and HTML to Markdown suitable for Gotify.

    Args:
        raw_description (str): The raw description to convert.

    Returns:
        str: The converted description.
    """
    # Bold
    raw_description = re.sub(
        r"\[b\](.*?)\[/b\]|<b>(.*?)</b>|<strong>(.*?)</strong>",
        r"**\1\2\3**",
        raw_description,
        flags=re.IGNORECASE,
    )

    # Italic
    raw_description = re.sub(
        r"\[i\](.*?)\[/i\]|<i>(.*?)</i>|<em>(.*?)</em>",
        r"*\1\2\3*",
        raw_description,
        flags=re.IGNORECASE,
    )

    # Underline
    raw_description = re.sub(
        r"\[u\](.*?)\[/u\]|<u>(.*?)</u>",
        r"__\1\2__",
        raw_description,
        flags=re.IGNORECASE,
    )

    # Strikethrough
    raw_description = re.sub(
        r"\[s\](.*?)\[/s\]|<s>(.*?)</s>|<strike>(.*?)</strike>",
        r"~~\1\2\3~~",
        raw_description,
        flags=re.IGNORECASE,
    )

    # Links
    def replace_link(match: re.Match[str]) -> str:
        if match.group(1) is not None:
            url = (match.group(1) or "").strip()
            text = (match.group(2) or "").strip()
        else:
            url = (match.group(3) or "").strip()
            text = (match.group(4) or "").strip()

        if not text or text == url or text.lower().startswith(("http://", "https://")):
            return url
        return f"[{text}]({url})"

    raw_description = re.sub(
        r'\[url=(.*?)\](.*?)\[/url\]|<a\s+[^>]*href=["\'](.*?)["\'][^>]*>(.*?)</a>',
        replace_link,
        raw_description,
        flags=re.IGNORECASE,
    )

    # Code
    raw_description = re.sub(
        r"\[code\](.*?)\[/code\]|<code>(.*?)</code>",
        r"`\1\2`",
        raw_description,
        flags=re.IGNORECASE,
    )

    # Remove any remaining BBcode or HTML code
    raw_description = re.sub(r"<.*?>", "", raw_description)
    raw_description = re.sub(r"\[(?![^\]]*\]\()([^\]]*)\]", "", raw_description)

    return raw_description
