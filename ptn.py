#!/usr/bin/env python3

import asyncio
import contextlib
import importlib
import pkgutil
from pathlib import Path
from typing import Any

import httpx
from rich.console import Console

console = Console()
try:
    import config
except ImportError:
    console.print("config.py not found. Creating from example-config.py...")
    console.print("Please edit config.py with your settings before running again.")
    import shutil

    shutil.copyfile("example-config.py", "config.py")
    exit(1)


TELEGRAM_BOT_TOKEN = config.SETTINGS.get("TELEGRAM_BOT_TOKEN")
TELEGRAM_CHAT_ID = config.SETTINGS.get("TELEGRAM_CHAT_ID")
TELEGRAM_TOPIC_ID = config.SETTINGS.get("TELEGRAM_TOPIC_ID")
CHECK_INTERVAL = config.SETTINGS.get("CHECK_INTERVAL", 900.0)
MARK_AS_READ = config.SETTINGS.get("MARK_AS_READ", False)


if not TELEGRAM_BOT_TOKEN or not TELEGRAM_CHAT_ID:
    console.print("Error: TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID must be set in .env")
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
            if not resp.is_success:
                console.print(f"Telegram Error: {resp.text}")
        except Exception as e:
            console.print(f"Telegram Exception: {e}")


COOKIES_DIR = Path("./cookies")
STATE_DIR = Path("./state")

STATE_DIR.mkdir(exist_ok=True)
COOKIES_DIR.mkdir(exist_ok=True)


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
                        console.print(f"Successfully loaded tracker: {module_name}")
                    else:
                        console.print(f"Tracker class {tracker_class_name} does not have a fetch_notifications method.")
                else:
                    if module_name != "base":
                        console.print(f"Tracker module {module_name} does not have a class named {tracker_class_name}.")
            except Exception as e:
                console.print(f"Failed to load tracker {tracker_info.name}: {e}")
    return trackers


async def main():
    """
    Main execution function that initializes trackers and runs the monitoring loop.
    """
    console.print("Starting PTNotifier...")
    trackers = load_trackers()

    while True:
        tasks = [tracker.fetch_notifications(send_telegram) for tracker in trackers.values()]
        if tasks:
            try:
                await asyncio.gather(*tasks)
            except Exception as e:
                console.print(f"An unexpected error occurred while gathering tracker tasks: {e}")
        else:
            console.print("No trackers loaded. Waiting...")

        minimum_interval = 900.0
        with contextlib.suppress(ValueError, TypeError):
            current_interval = float(CHECK_INTERVAL) if minimum_interval < CHECK_INTERVAL else 900.0

        console.print(f"Sleeping for {current_interval / 60.0} minutes...")
        try:
            await asyncio.sleep(current_interval)
        except Exception as e:
            console.print(f"An unexpected error occurred during sleep: {e}")
            await asyncio.sleep(60)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        console.print("PTNotifier stopped by user.")
