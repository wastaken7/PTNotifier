#!/usr/bin/env python3

import asyncio
import contextlib
import glob
import importlib
import pkgutil
import subprocess
from pathlib import Path
from typing import Any

import httpx
from rich.console import Console


async def check_version():
    """Checks for new versions of the script on GitHub."""
    try:
        local_hash = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()  # noqa: ASYNC221

        async with httpx.AsyncClient() as client:
            resp = await client.get("https://api.github.com/repos/wastaken7/PTNotifier/commits/main")
            resp.raise_for_status()
            remote_hash = resp.json()["sha"]

        if local_hash != remote_hash:
            console.print("[bold yellow]A new version is available. Please update your script.[/bold yellow]")

    except Exception as e:
        console.print(f"[bold red]Version check failed:[/] {e}")


console = Console()
try:
    import config
except ImportError:
    console.print("[yellow]config.py not found. Creating from example-config.py...[/yellow]")
    console.print("[yellow]Please edit config.py with your settings before running again.[/yellow]")
    import shutil

    shutil.copyfile("example-config.py", "config.py")
    exit(1)

COOKIES_DIR = Path("./cookies")
STATE_DIR = Path("./state")
STATE_DIR.mkdir(exist_ok=True)
COOKIES_DIR.mkdir(exist_ok=True)

TELEGRAM_BOT_TOKEN = config.SETTINGS.get("TELEGRAM_BOT_TOKEN")
TELEGRAM_CHAT_ID = config.SETTINGS.get("TELEGRAM_CHAT_ID")
TELEGRAM_TOPIC_ID = config.SETTINGS.get("TELEGRAM_TOPIC_ID")
CHECK_INTERVAL = config.SETTINGS.get("CHECK_INTERVAL", 900.0)
MARK_AS_READ = config.SETTINGS.get("MARK_AS_READ", False)


if not TELEGRAM_BOT_TOKEN or not TELEGRAM_CHAT_ID:
    console.print("Error: [bold red]TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID must be set in config.py[/bold red]")
    exit(1)


async def send_telegram(item: dict[str, str], domain: str, notifications_url: str):
    """
    Sends a formatted notification to Telegram.
    """
    if item["type"] == "notification":
        text = (
            f"🔔 <b>{domain.upper()}</b>\n\n"
            f"📌 <b>{item['title']}</b>\n\n"
            f"<b>Message:</b> {item['msg']}\n\n"
            f"{item['date']}"  # fmt: skip
        )

    elif item["type"] == "message":
        staff_tag = "⚠ <b>STAFF MESSAGE</b> ⚠\n\n" if item.get("is_staff") else ""
        body = f"<b>Body:</b> {item.get('body', '')}\n\n" if item.get("body") else ""

        text = (
            f"📩 <b>{domain.upper()}</b>\n\n"
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


def load_trackers() -> dict[str, Any]:
    """Dynamically loads all tracker classes from the 'trackers' directory."""
    console.print("Loading trackers...")
    trackers: dict[str, Any] = {}
    tracker_modules = pkgutil.iter_modules([str(Path("trackers"))])
    for tracker_info in tracker_modules:
        if not tracker_info.ispkg:
            try:
                module_name = tracker_info.name
                module = importlib.import_module(f"trackers.{module_name}")
                tracker_class_name = f"{module_name}"
                if hasattr(module, tracker_class_name):
                    tracker_class = getattr(module, tracker_class_name)
                    if hasattr(tracker_class, "fetch_notifications"):
                        trackers[module_name] = tracker_class
                    else:
                        console.print(f"[bold red]Tracker class {tracker_class_name} does not have a fetch_notifications method.[/bold red]")
                else:
                    if module_name != "base":
                        console.print(f"[bold red]Tracker module {module_name} does not have a class named {tracker_class_name}.[/bold red]")
            except Exception as e:
                console.print(f"[bold red]Failed to load tracker {tracker_info.name}:[/bold red] {e}")
    return trackers


async def main():
    """
    Main execution function that initializes trackers and runs the monitoring loop.
    """
    await check_version()
    console.print("Starting PTNotifier...")
    tracker_classes = load_trackers()

    while True:
        tasks = []
        for tracker_name, tracker_class in tracker_classes.items():
            cookie_files = glob.glob(str(COOKIES_DIR / tracker_name.upper() / "*.txt"))
            if not cookie_files:
                cookie_files = glob.glob(str(COOKIES_DIR / tracker_name / "*.txt"))

            for f in cookie_files:
                tracker_instance = tracker_class(Path(f))
                tasks.append(tracker_instance.fetch_notifications(send_telegram))

        # Handle OTHER directory
        other_cookie_files = glob.glob(str(COOKIES_DIR / "OTHER" / "*.txt"))
        for f in other_cookie_files:
            cookie_path = Path(f)
            tracker_name_from_file = cookie_path.stem
            tracker_class = tracker_classes.get(tracker_name_from_file)
            if tracker_class:
                tracker_instance = tracker_class(cookie_path)
                tasks.append(tracker_instance.fetch_notifications(send_telegram))
            else:
                console.print(f"[bold red]No tracker class found for cookie file: {cookie_path.name}[/bold red]")

        if tasks:
            try:
                await asyncio.gather(*tasks)
            except Exception as e:
                console.print(f"[bold red]An unexpected error occurred while gathering tracker tasks:[/bold red] {e}")
        else:
            console.print("[yellow]No trackers loaded. Waiting...[/yellow]")

        minimum_interval = 900.0
        current_interval = minimum_interval
        with contextlib.suppress(ValueError, TypeError):
            interval_from_config = float(CHECK_INTERVAL)
            if interval_from_config > minimum_interval:
                current_interval = interval_from_config

        console.print(f"[green]Sleeping for {current_interval / 60.0} minutes...[/green]")
        try:
            await asyncio.sleep(current_interval)
        except Exception as e:
            console.print(f"[bold red]An unexpected error occurred during sleep:[/bold red] {e}")
            await asyncio.sleep(60)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        console.print("PTNotifier stopped by user.")
