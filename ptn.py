#!/usr/bin/env python3

import asyncio
import glob
import sys
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


async def main():
    """
    Main execution function that initializes trackers and runs the monitoring loop.
    """
    await check_version()
    tracker_classes = load_trackers()

    cookies_dir = Path("./cookies")

    notifiers: list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]] = []
    if telegram_bot_token and telegram_chat_id:
        notifiers.append(send_telegram)
    if discord_webhook_url:
        notifiers.append(send_discord)

    while True:
        tasks: list[Coroutine[Any, Any, float]] = []

        for tracker_name, tracker_class in tracker_classes.items():
            search_patterns = [cookies_dir / tracker_name.upper() / "*.txt", cookies_dir / tracker_name / "*.txt", cookies_dir / "Other" / f"{tracker_name}.txt"]

            seen_files: set[Path] = set()
            for pattern in search_patterns:
                for cookie_file in glob.glob(str(pattern)):
                    path_obj = Path(cookie_file)
                    if path_obj not in seen_files:
                        tasks.append(tracker_class(path_obj).fetch_notifications(notifiers))
                        seen_files.add(path_obj)

        if not tasks:
            log.warning("No tracker tasks found. Waiting 60s...")
            await asyncio.sleep(60)
            continue

        results: list[float] = []
        with Progress() as progress:
            main_task = progress.add_task("[bold blue]Processing trackers...", total=len(tasks))

            for future in asyncio.as_completed(tasks):
                try:
                    result = await future
                    if result and result > 0:
                        results.append(result)
                except Exception as e:  # noqa: PERF203
                    log.error("Tracker execution failed:", exc_info=e)
                finally:
                    progress.update(main_task, advance=1)

        sleep_interval = min(results) if results else 60

        time_str = f"{sleep_interval:.2f} seconds" if sleep_interval <= 60 else f"{sleep_interval / 60:.2f} minutes"

        log.info(f"Cycle complete. Next check in {time_str}.")

        try:
            await asyncio.sleep(sleep_interval)
        except asyncio.CancelledError:
            log.info("Monitoring stopped by user.")
            break
        except Exception as e:
            log.error(f"Unexpected error during sleep: {e}")
            await asyncio.sleep(60)

        # Clear the last two lines of the console
        sys.stdout.write("\033[2A\033[J")
        sys.stdout.flush()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        log.info("PTNotifier stopped by user.")
