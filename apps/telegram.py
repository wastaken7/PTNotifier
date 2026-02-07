import httpx
from rich.console import Console

import config

console = Console()

TELEGRAM_BOT_TOKEN = config.SETTINGS.get("TELEGRAM_BOT_TOKEN")
TELEGRAM_CHAT_ID = config.SETTINGS.get("TELEGRAM_CHAT_ID")
TELEGRAM_TOPIC_ID = config.SETTINGS.get("TELEGRAM_TOPIC_ID")

async def send_telegram(item: dict[str, str], domain: str, notifications_url: str):
    """
    Sends a formatted notification to Telegram.
    """
    if item["type"] == "notification":
        text = (
            f"🔔 <b>{domain}</b>\n\n"
            f"📌 <b>{item['title']}</b>\n\n"
            f"<b>Message:</b> {item['msg']}\n\n"
            f"{item['date']}"  # fmt: skip
        )

    elif item["type"] == "message":
        staff_tag = "⚠ <b>STAFF MESSAGE</b> ⚠\n\n" if item.get("is_staff") else ""
        body = f"<b>Body:</b> {item.get('body', '')}\n\n" if item.get("body") else ""

        text = (
            f"📩 <b>{domain}</b>\n\n"
            f"{staff_tag}"
            f"👤 <b>{item['title']}</b>\n\n"
            f"<b>Text:</b> {item['msg']}\n\n"
            f"{body}"
            f"{item['date']}"  # fmt: skip
        )

    keyboard = {"inline_keyboard": [[{"text": "Open Notification", "url": notifications_url}]]}

    payload = {"chat_id": TELEGRAM_CHAT_ID, "text": text, "parse_mode": "HTML", "reply_markup": keyboard}

    if TELEGRAM_TOPIC_ID:
        payload["message_thread_id"] = TELEGRAM_TOPIC_ID

    async with httpx.AsyncClient() as tg_client:
        try:
            url = f"https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendMessage"
            resp = await tg_client.post(url, json=payload)
            resp.raise_for_status()
            if not resp.is_success:
                console.print(f"[bold red]Telegram Error[/bold red]: {resp.text}")
        except Exception as e:
            console.print(f"[bold red]Telegram Exception[/bold red]: {e}")
