from typing import Any

import httpx
from rich.console import Console

import config

console = Console()


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
        console.print("[bold red]TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID not set in config.py.[/bold red]")
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
        content += f"<b>Body:</b> {item['body']}\n\n"

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
                console.print(f"[bold red]Telegram Error[/bold red]: {resp.text}")
        except Exception as e:
            console.print(f"[bold red]Telegram Exception[/bold red]: {e}")
