import httpx
from rich.console import Console

import config

console = Console()
DISCORD_WEBHOOK_URL = config.SETTINGS.get("DISCORD_WEBHOOK_URL")

async def send_discord(item: dict[str, str], domain: str, notifications_url: str):
    """
    Sends a formatted notification to Discord via Webhook.
    """
    icon = "🔔" if item["type"] == "notification" else "📩"

    description = f"### {icon} {domain}\n"

    if item.get("is_staff"):
        description += "⚠️ **STAFF MESSAGE** ⚠️\n\n"

    description += f"**{item['title']}**\n\n{item['msg']}\n"

    if item.get("body"):
        description += f"\n**Body:**\n{item['body']}\n"

    description += f"\n[Open Notification]({notifications_url})"

    payload = {
        "embeds": [
            {
                "description": description,
                "color": 0x5865F2,
                "footer": {"text": item["date"]},
            }
        ]
    }

    async with httpx.AsyncClient() as client:
        try:
            resp = await client.post(DISCORD_WEBHOOK_URL, json=payload)
            resp.raise_for_status()
        except Exception as e:
            console.print(f"[bold red]Discord Exception[/bold red]: {e}")
