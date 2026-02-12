#!/usr/bin/env python3

import asyncio
import glob
import importlib
import logging
import pkgutil
import subprocess
from collections.abc import Coroutine
from pathlib import Path
from typing import Any, Callable

import httpx
from rich.console import Console
from rich.logging import RichHandler
from rich.progress import Progress

from apps.discord import send_discord
from apps.telegram import send_telegram

console = Console()

logging.getLogger("httpx").setLevel(logging.WARNING)
logging.getLogger("httpcore").setLevel(logging.WARNING)
clean_handler = RichHandler(
    show_time=False,
    show_level=False,
    show_path=False,
    markup=True,
)
logging.basicConfig(
    level=logging.INFO,
    format="%(message)s",
    handlers=[clean_handler],
)
log = logging.getLogger("rich")
try:
    import config as _imported_config
except ImportError:
    log.warning("config.py not found. Creating from example-config.py...")
    log.warning("Please edit config.py with your settings before running again.")
    import shutil

    shutil.copyfile("example-config.py", "config.py")
    exit(1)

try:
    user_config: dict[str, Any] = _imported_config.SETTINGS
    api_tokens: dict[str, str] = _imported_config.API_TOKENS
except Exception as e:
    log.error("Error loading config.py:", exc_info=e)
    log.error("Check example-config.py for any missing fields.")
    exit(1)

COOKIES_DIR = Path("./cookies")
STATE_DIR = Path("./state")
STATE_DIR.mkdir(exist_ok=True)
COOKIES_DIR.mkdir(exist_ok=True)
TELEGRAM_BOT_TOKEN = user_config.get("TELEGRAM_BOT_TOKEN")
TELEGRAM_CHAT_ID = user_config.get("TELEGRAM_CHAT_ID")
DISCORD_WEBHOOK_URL = user_config.get("DISCORD_WEBHOOK_URL")
CHECK_INTERVAL = user_config.get("CHECK_INTERVAL", 900.0)
MARK_AS_READ = user_config.get("MARK_AS_READ", False)


async def check_version():
    """Checks for new versions of the script on GitHub.s"""
    try:
        log.info("Checking for new version...")
        local_hash = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()  # noqa: ASYNC221

        async with httpx.AsyncClient() as client:
            resp = await client.get("https://api.github.com/repos/wastaken7/PTNotifier/commits/main")
            resp.raise_for_status()
            remote_hash = resp.json()["sha"]

        if local_hash != remote_hash:
            log.warning("A new update is available on main branch.")
            log.info("Run 'git pull' to stay up to date.\n")
        else:
            log.info("No new updates found.")

    except Exception as e:
        log.error("Version check failed:", exc_info=e)
        log.warning("Make sure git is installed and you have internet connectivity.")


if (not TELEGRAM_BOT_TOKEN or not TELEGRAM_CHAT_ID) and not DISCORD_WEBHOOK_URL:
    log.error("Please set TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID or DISCORD_WEBHOOK_URL in config.py")
    exit(1)


def load_trackers() -> dict[str, Any]:
    """Dynamically loads all tracker classes from the 'trackers' directory."""
    log.info("Loading trackers...")
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
                        log.error(f"Tracker class {tracker_class_name} does not have a fetch_notifications method.")
                else:
                    if module_name != "base":
                        log.error(f"Tracker module {module_name} does not have a class named {tracker_class_name}.")
            except Exception as e:
                log.error(f"Failed to load tracker {tracker_info.name}:", exc_info=e)
    return trackers


async def main():
    """
    Main execution function that initializes trackers and runs the monitoring loop.
    """
    await check_version()
    tracker_classes = load_trackers()

    notifiers: list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]] = []

    if TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID:
        notifiers.append(send_telegram)
    if DISCORD_WEBHOOK_URL:
        notifiers.append(send_discord)

    while True:
        tasks: list[Any] = []
        for tracker_name, tracker_class in tracker_classes.items():
            cookie_files = glob.glob(str(COOKIES_DIR / tracker_name.upper() / "*.txt"))
            if not cookie_files:
                cookie_files = glob.glob(str(COOKIES_DIR / tracker_name / "*.txt"))

            for f in cookie_files:
                tracker_instance = tracker_class(Path(f))
                tasks.append(tracker_instance.fetch_notifications(notifiers))

        # Handle Other directory
        other_cookie_files: list[str] = glob.glob(str(COOKIES_DIR / "Other" / "*.txt"))
        for f in other_cookie_files:
            cookie_path = Path(f)
            tracker_name_from_file = cookie_path.stem
            tracker_class = tracker_classes.get(tracker_name_from_file)
            if tracker_class:
                tracker_instance = tracker_class(cookie_path)
                tasks.append(tracker_instance.fetch_notifications(notifiers))
            else:
                log.error(f"No tracker class found for cookie file: {cookie_path.name}")

        sleep_time: float
        if tasks:
            with Progress() as progress:
                task_id = progress.add_task("[bold blue]Checking trackers...", total=len(tasks))
                results: list[Any] = []
                for task in asyncio.as_completed(tasks):
                    try:
                        result = await task
                        results.append(result)
                    except Exception as e:
                        log.error("An error occurred while processing a tracker:", exc_info=e)
                    progress.update(task_id, advance=1)

            # Filter out exceptions and non-float values
            valid_times: list[float] = [t for t in results if isinstance(t, (int, float)) and t > 0]
            sleep_time = min(valid_times) if valid_times else 60
        else:
            log.warning("No trackers loaded. Waiting...")
            sleep_time = 60

        sleep_time_print = f"{sleep_time:.2f} seconds" if sleep_time <= 60 else f"{sleep_time / 60:.2f} minute{'s' if sleep_time / 60 != 1 else ''}"

        log.info(f"Waiting {sleep_time_print} before checking again...")
        try:
            await asyncio.sleep(sleep_time)
        except Exception as e:
            log.error("An unexpected error occurred during sleep:", exc_info=e)
            await asyncio.sleep(sleep_time)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        log.info("PTNotifier stopped by user.")
