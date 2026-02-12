#!/usr/bin/env python3

import asyncio
import glob
from collections.abc import Coroutine
from pathlib import Path
from typing import Any, Callable

from rich.progress import Progress

from apps.discord import send_discord
from apps.telegram import send_telegram
from utils.check_version import check_version
from utils.config_validator import load_config
from utils.console import log
from utils.tracker_loader import load_trackers

user_config, api_tokens, discord_webhook_url, telegram_bot_token, telegram_chat_id = load_config()

COOKIES_DIR = Path("./cookies")
STATE_DIR = Path("./state")
STATE_DIR.mkdir(exist_ok=True)
COOKIES_DIR.mkdir(exist_ok=True)


async def main():
    """
    Main execution function that initializes trackers and runs the monitoring loop.
    """
    await check_version()
    tracker_classes = load_trackers()

    notifiers: list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]] = []

    if telegram_bot_token and telegram_chat_id:
        notifiers.append(send_telegram)
    if discord_webhook_url:
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
                task_id = progress.add_task("[bold blue]Processing...", total=len(tasks))
                results: list[float] = []
                for task in asyncio.as_completed(tasks):
                    try:
                        result = await task
                        results.append(result)
                    except Exception as e:
                        log.error("An error occurred while processing a tracker:", exc_info=e)
                    progress.update(task_id, advance=1)

            # Filter out exceptions and non-float values
            valid_times: list[float] = [t for t in results if t > 0]
            sleep_time = min(valid_times) if valid_times else 60
        else:
            log.warning("No trackers loaded. Waiting...")
            sleep_time = 60

        sleep_time_print = f"{sleep_time:.2f} seconds" if sleep_time <= 60 else f"{sleep_time / 60:.2f} minute{'s' if sleep_time / 60 != 1 else ''}"

        log.info(f"Waiting {sleep_time_print} before checking again...\n\n")
        try:
            await asyncio.sleep(sleep_time)
        except Exception as e:
            log.error("An unexpected error occurred during sleep:", exc_info=e)
            await asyncio.sleep(sleep_time)
        log.info("Checking again...\n")


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        log.info("PTNotifier stopped by user.")
