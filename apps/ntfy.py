import re
from typing import Any

import httpx

import config
from utils.console import log


async def send_ntfy(
    item: dict[str, Any],
    tracker_name: str,
    _base_url: str,
    notifications_url: str,
) -> None:
    """Sends a formatted notification to ntfy.

    Args:
        item (dict[str, Any]): The item to send.
        tracker_name (str): The name of the tracker.
        _base_url (str): The base URL of the tracker.
        notifications_url (str): The URL to the notifications page.

    Returns:
        None
    """
    ntfy_url: str = config.SETTINGS.get("NTFY_URL", "").rstrip("/")
    ntfy_topic: str = config.SETTINGS.get("NTFY_TOPIC", "")
    ntfy_token: str = config.SETTINGS.get("NTFY_TOKEN", "")
    ntfy_priority: int = config.SETTINGS.get("NTFY_PRIORITY", 3)

    if not ntfy_url or not ntfy_topic:
        log.error("NTFY_URL or NTFY_TOPIC not set in config.py.")
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
        clean_body = format_for_ntfy(item.get("body", ""))
        content += f"**Body:** {clean_body}\n\n"

    content += f"**Date:** {item.get('date', '')}\n\n"

    payload: dict[str, Any] = {
        "topic": ntfy_topic,
        "title": title,
        "message": content,
        "priority": ntfy_priority,
        "markdown": True,
        "click": notifications_url,
    }

    url = f"{ntfy_url}"

    headers: dict[str, str] = {}
    if ntfy_token:
        headers["Authorization"] = f"Bearer {ntfy_token}"

    async with httpx.AsyncClient() as client:
        try:
            resp = await client.post(url, json=payload, headers=headers)
            resp.raise_for_status()
            if not resp.is_success:
                log.error(f"ntfy Error: {resp.text}")
        except Exception as e:
            log.error(f"ntfy Exception: {e}")
            log.debug("ntfy error details", exc_info=True)


def format_for_ntfy(raw_description: str) -> str:
    """Converts BBCode and HTML to Markdown suitable for ntfy.

    Args:
        raw_description (str): The raw description to convert.

    Returns:
        str: The converted description.
    """
    # Bold
    raw_description = re.sub(r"\[b\](.*?)\[/b\]|<b>(.*?)</b>|<strong>(.*?)</strong>", r"**\1\2\3**", raw_description, flags=re.IGNORECASE)

    # Italic
    raw_description = re.sub(r"\[i\](.*?)\[/i\]|<i>(.*?)</i>|<em>(.*?)</em>", r"*\1\2\3*", raw_description, flags=re.IGNORECASE)

    # Underline
    raw_description = re.sub(r"\[u\](.*?)\[/u\]|<u>(.*?)</u>", r"__\1\2__", raw_description, flags=re.IGNORECASE)

    # Strikethrough
    raw_description = re.sub(r"\[s\](.*?)\[/s\]|<s>(.*?)</s>|<strike>(.*?)</strike>", r"~~\1\2\3~~", raw_description, flags=re.IGNORECASE)

    # Links
    raw_description = re.sub(r'\[url=(.*?)\](.*?)\[/url\]|<a href="(.*?)">(.*?)</a>', r"[\2\4](\1\3)", raw_description, flags=re.IGNORECASE)

    # Code
    raw_description = re.sub(r"\[code\](.*?)\[/code\]|<code>(.*?)</code>", r"`\1\2`", raw_description, flags=re.IGNORECASE)

    # Remove any remaining BBcode or HTML code
    raw_description = re.sub(r"<.*?>", "", raw_description)
    raw_description = re.sub(r"\[.*?\]", "", raw_description)

    return raw_description
