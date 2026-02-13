import re
from typing import Any

import httpx

import config
from trackers.base import log


async def send_telegram(
    item: dict[str, str],
    tracker_name: str,
    _base_url: str,
    notifications_url: str,
) -> None:
    """
    Sends a formatted notification to Telegram.
    """
    telegram_bot_token: str = config.SETTINGS.get("TELEGRAM_BOT_TOKEN", "")
    telegram_chat_id: str = config.SETTINGS.get("TELEGRAM_CHAT_ID", "")
    telegram_topic_id: str = config.SETTINGS.get("TELEGRAM_TOPIC_ID", "")
    if not telegram_bot_token or not telegram_chat_id:
        log.error("TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID not set in config.py.")
        return

    icon = "🔔" if item["type"] == "notification" else "📩"

    header = f"<b>{tracker_name}</b>\n\n"
    header += f"<b>{icon} New {item['type'].capitalize()}</b>\n\n"

    content = ""
    if item.get("is_staff"):
        content += "⚠️ <b>STAFF MESSAGE</b> ⚠️\n\n"

    if item.get("sender"):
        content += f"👤 {item['sender']}\n\n"

    if item.get("title"):
        content += f"<b>Title:</b> {item['title']}\n\n"

    if item.get("subject"):
        content += f"<b>Subject:</b> {item['subject']}\n\n"

    if item.get("body"):
        clean_body = format_for_telegram(item.get("body", ""))
        content += f"<b>Body:</b> {clean_body}\n\n"

    footer = item["date"]

    text = f"{header}{content}{footer}"

    keyboard = {"inline_keyboard": [[{"text": "Open Notification", "url": notifications_url}]]}

    payload: dict[str, Any] = {
        "chat_id": telegram_chat_id,
        "text": text,
        "parse_mode": "HTML",
        "reply_markup": keyboard,
        "disable_web_page_preview": True,
    }

    if telegram_topic_id:
        payload["message_thread_id"] = telegram_topic_id

    async with httpx.AsyncClient() as tg_client:
        try:
            url: str = f"https://api.telegram.org/bot{telegram_bot_token}/sendMessage"
            resp = await tg_client.post(url, json=payload)
            resp.raise_for_status()
            if not resp.is_success:
                log.error(f"Telegram Error: {resp.text}")
        except Exception as e:
            log.error(f"Telegram Exception: {e}")
            log.debug("Telegram error details", exc_info=True)

def format_for_telegram(text: str) -> str:
    """
    Converts BBCode and generic HTML to Telegram-compatible HTML.
    """
    # 1. Normalize BBCode to HTML
    text = re.sub(r"\[b\](.*?)\[/b\]", r"<b>\1</b>", text, flags=re.IGNORECASE)
    text = re.sub(r"\[i\](.*?)\[/i\]", r"<i>\1</i>", text, flags=re.IGNORECASE)
    text = re.sub(r"\[u\](.*?)\[/u\]", r"<u>\1</u>", text, flags=re.IGNORECASE)
    text = re.sub(r"\[s\](.*?)\[/s\]", r"<s>\1</s>", text, flags=re.IGNORECASE)
    text = re.sub(r"\[spoiler\](.*?)\[/spoiler\]", r"<tg-spoiler>\1</tg-spoiler>", text, flags=re.IGNORECASE)
    text = re.sub(r"\[url=(.*?)\](.*?)\[/url\]", r'<a href="\1">\2</a>', text, flags=re.IGNORECASE)
    text = re.sub(r"\[code\](.*?)\[/code\]", r"<code>\1</code>", text, flags=re.IGNORECASE)

    # 2. Convert specific HTML tags to Telegram's unique tags
    text = re.sub(r"<spoiler>(.*?)</spoiler>", r"<tg-spoiler>\1</tg-spoiler>", text, flags=re.IGNORECASE)
    text = re.sub(r"<strike>(.*?)</strike>", r"<s>\1</s>", text, flags=re.IGNORECASE)

    # 3. Final cleaning: Telegram HTML is strict, but usually accepts <b>, <i>, <u>, <s>, <a> and <code>
    return text
