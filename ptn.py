#!/usr/bin/env python3

import asyncio
import glob
import sys
from pathlib import Path
from typing import Any

from rich.progress import Progress

from utils.check_version import check_version
from utils.config_validator import load_config
from utils.console import log
from utils.tracker_loader import load_trackers

user_config, api_tokens, discord_webhook_url, telegram_bot_token, telegram_chat_id = load_config()


async def main():
    """
    Main execution function that initializes trackers and runs the monitoring loop.
    """
    tracker_classes = load_trackers()
    cookies_dir = Path("./cookies")

    while True:
        await check_version()
        tasks: list[asyncio.Task[Any] | Any] = []

        for tracker_name, tracker_class in tracker_classes.items():
            search_patterns = [
                cookies_dir / tracker_name.upper() / "*.txt",
                cookies_dir / tracker_name / "*.txt",
                cookies_dir / "Other" / f"{tracker_name}.txt",
            ]

            seen_files: set[Path] = set()
            for pattern in search_patterns:
                for cookie_file in glob.glob(str(pattern)):
                    path_obj = Path(cookie_file)
                    if path_obj in seen_files:
                        continue

                    tracker_instance = tracker_class(path_obj)
                    domain = tracker_instance._extract_domain_from_cookie(path_obj)
                    if domain:
                        tracker_name = tracker_instance.get_tracker_name(domain)

                    async def wrapped_task(t_name: str = tracker_name, inst: Any = tracker_instance) -> tuple[str, Any]:
                        try:
                            res = await inst.fetch_notifications()
                            return t_name, res
                        except Exception as e:
                            log.error(f"{t_name}: Tracker execution failed.")
                            log.debug(f"{t_name}: Error details: {e}", exc_info=True)
                            return t_name, None

                    tasks.append(wrapped_task())  # type: ignore
                    seen_files.add(path_obj)

        if not tasks:
            log.warning("No tracker tasks found. Waiting 60s...")
            await asyncio.sleep(60)
            continue

        total_tasks = len(tasks)
        results: list[float] = []
        completed_count = 0

        with Progress() as progress:
            main_task = progress.add_task("[bold blue]Initializing...", total=total_tasks)

            for future in asyncio.as_completed(tasks):
                completed_count += 1
                tracker_name, result = await future

                progress.update(main_task, description=f"[bold blue]Processing {tracker_name} [[cyan]{completed_count:02d}/{total_tasks:02d}[/cyan]]", advance=1)

                if result and result > 0:
                    results.append(result)

        sleep_interval = min(results) if results else 60
        time_str = f"{sleep_interval:.2f} seconds" if sleep_interval <= 60 else f"{sleep_interval / 60:.2f} minutes"

        log.info(f"Cycle complete. Next check in {time_str}.")

        try:
            await asyncio.sleep(sleep_interval)
        except asyncio.CancelledError:
            log.info("Monitoring stopped by user.")
            break

        # Clear lines
        sys.stdout.write("\033[2A\033[J")
        sys.stdout.flush()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        log.info("PTNotifier stopped by user.")
