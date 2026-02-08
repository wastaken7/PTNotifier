import json
from typing import Any, Optional

import aiofiles
import httpx
from anyio import Path
from PIL import Image
from rich.console import Console

import config

console = Console()


async def get_local_favicon(client: httpx.AsyncClient, icon_url: str, tracker_name: str) -> tuple[Optional[Path], str]:
    """
    Downloads the favicon from a URL, converts it to PNG, and saves it.
    Returns a tuple containing the local Path (if successful) and the filename.
    """
    icons_dir = Path("state") / "favicon"
    await icons_dir.mkdir(parents=True, exist_ok=True)

    safe_name = "".join(c for c in tracker_name if c.isalnum() or c in (" ", "_", "-")).strip()
    icon_filename = f"{safe_name}_icon.png"
    icon_path = icons_dir / icon_filename

    if await icon_path.exists():
        return icon_path, icon_filename

    try:
        resp = await client.get(icon_url, follow_redirects=True)
        if resp.status_code == 200:
            temp_ico_path = icon_path.with_suffix(".ico")
            async with aiofiles.open(str(temp_ico_path), "wb") as f:
                await f.write(resp.content)

            # Convert ICO to PNG
            # Discord webhooks work better with PNG attachments for icons.
            with Image.open(temp_ico_path) as img:
                img.save(icon_path, "PNG")

            await temp_ico_path.unlink()

            return icon_path, icon_filename
    except Exception as e:
        console.print(f"[yellow]Warning: Could not download or convert icon from {icon_url}: {e}[/yellow]")

    return None, icon_filename


async def send_discord(
    item: dict[str, str],
    tracker_name: str,
    base_url: str,
    notifications_url: str,
) -> None:
    """
    Main function to send a formatted notification to Discord via Webhook.
    """
    webhook_url: str = config.SETTINGS.get("DISCORD_WEBHOOK_URL", "")
    if not webhook_url:
        console.print("[bold red]DISCORD_WEBHOOK_URL not set in config.py.[/bold red]")
        return

    icon_url = item.get("favicon", f"{base_url}/favicon.ico")

    async with httpx.AsyncClient() as client:
        # 1. Handle favicon download using async path methods
        icon_path, icon_filename = await get_local_favicon(client, icon_url, tracker_name)

        # 2. Build description
        icon_emoji = "🔔" if item["type"] == "notification" else "📩"
        description = f"{icon_emoji} **New {item['type'].capitalize()}**\n\n\n"

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

        embed_icon = f"attachment://{icon_filename}" if icon_path else icon_url

        payload: dict[str, Any] = {
            "embeds": [
                {
                    "description": description,
                    "color": 0xFE0203 if item["type"] == "notification" else 0x5865F2,
                    "footer": {"text": item["date"]},
                    "author": {
                        "name": tracker_name,
                        "icon_url": embed_icon,
                    },
                }
            ]
        }

        try:
            # Async check if file exists
            if icon_path and await icon_path.exists():
                async with aiofiles.open(str(icon_path), "rb") as f:
                    content = await f.read()
                    files = {"file": (icon_filename, content, "image/png")}
                    resp = await client.post(webhook_url, data={"payload_json": json.dumps(payload)}, files=files)
            else:
                resp = await client.post(webhook_url, json=payload)

            resp.raise_for_status()
        except Exception as e:
            console.print(f"[bold red]Discord Exception[/bold red]: {e}")
