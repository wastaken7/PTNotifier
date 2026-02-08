from typing import Any

import httpx
from rich.console import Console

import config

console = Console()


async def send_discord(
    item: dict[str, str],
    tracker_name: str,
    base_url: str,
    notifications_url: str,
) -> None:
    """
    Sends a formatted notification to Discord via Webhook.
    """
    discord_webhook_url: str = config.SETTINGS.get("DISCORD_WEBHOOK_URL", "")
    if not discord_webhook_url:
        console.print("[bold red]DISCORD_WEBHOOK_URL not set in config.py.[/bold red]")
        return

    icon = "🔔" if item["type"] == "notification" else "📩"

    description = f"{icon} **New {item['type'].capitalize()}**\n\n\n"

    if item.get("is_staff"):
        description += "⚠️ **STAFF MESSAGE** ⚠️\n\n"

    if item.get("sender"):
        description += f"👤  {item['sender']}\n\n"

    if item.get("title"):
        description += f"**Title:** {item['title']}\n\n"

    if item.get("subject"):
        description += f"**Subject: **{item['subject']}\n\n"

    if item.get("body"):
        description += f"**Body:** {item['body']}\n\n"

    description += f"[Open Notification]({notifications_url})"

    icon_url = item.get("favicon", f"{base_url}/favicon.ico")

    payload: dict[str, Any] = {
        "embeds": [
            {
                "description": description,
                "color": 0xFE0203 if item["type"] == "notification" else 0x5865F2,
                "footer": {"text": item["date"]},
                "author": {
                    "name": tracker_name,
                    "icon_url": icon_url,
                },
            }
        ]
    }

    async with httpx.AsyncClient() as client:
        try:
            resp = await client.post(discord_webhook_url, json=payload)
            resp.raise_for_status()
        except Exception as e:
            console.print(f"[bold red]Discord Exception[/bold red]: {e}")
