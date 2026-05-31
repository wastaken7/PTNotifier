#!/usr/bin/env python3

import argparse
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

(
    user_config,
    api_tokens,
    discord_webhook_url,
    telegram_bot_token,
    telegram_chat_id,
    gotify_url,
    gotify_token,
    ntfy_url,
    ntfy_topic,
) = load_config()


def parse_args() -> argparse.Namespace:
    """
    Parse command line arguments.
    """
    parser = argparse.ArgumentParser(description="PTNotifier")
    parser.add_argument(
        "--test-notification",
        nargs="?",
        const="__FIRST__",
        metavar="TRACKER",
        help="Send test notifications. Optionally target one or more trackers, e.g. --test-notification LST,Anthelion",
    )
    return parser.parse_args()


def parse_test_notification_targets(raw_target: str | None) -> list[str] | None:
    """
    Parse the target trackers for test notifications.

    Args:
        raw_target: Raw target string from command line arguments.

    Returns:
        List of target trackers or None if no target is specified.
    """
    if raw_target is None:
        return None

    targets = [target.strip() for target in raw_target.split(",") if target.strip()]
    return targets or None


def iter_tracker_instances(tracker_classes: dict[str, Any]) -> list[tuple[str, Any]]:
    """
    Iterate through tracker instances based on cookie files.

    Args:
        tracker_classes: Dictionary of tracker classes.

    Returns:
        List of tracker instances.
    """
    cookies_dir = Path("./cookies")
    instances: list[tuple[str, Any]] = []

    for tracker_name, tracker_class in tracker_classes.items():
        if getattr(tracker_class, "api_only", False):
            if not api_tokens.get(tracker_name):
                continue

            tracker_instance = tracker_class(Path("./cookies") / f"{tracker_name}.txt")
            instances.append((tracker_name, tracker_instance))
            continue

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
                instances.append((tracker_name, tracker_instance))
                seen_files.add(path_obj)

    return instances


async def run_test_notification(tracker_classes: dict[str, Any], target_trackers: list[str] | None) -> int:
    """
    Run test notifications for the specified trackers.

    Args:
        tracker_classes: Dictionary of tracker classes.
        target_trackers: List of target trackers.

    Returns:
        0 if successful, 1 otherwise.
    """
    candidates = iter_tracker_instances(tracker_classes)
    if target_trackers:
        normalized_targets = {target.lower() for target in target_trackers}
        candidates = [
            (tracker_name, tracker_instance)
            for tracker_name, tracker_instance in candidates
            if normalized_targets.intersection(
                {
                    tracker_name.lower(),
                    tracker_instance.tracker.lower(),
                    tracker_instance.__class__.__name__.lower(),
                    getattr(tracker_instance, "domain", "").lower(),
                    Path(getattr(tracker_instance, "filename", "")).stem.lower(),
                }
            )
        ]

    if not candidates:
        if target_trackers:
            log.error(f"No tracker instance found for test notification target(s): {', '.join(target_trackers)}")
        else:
            log.error("No tracker instances found for test notification.")
        return 1

    if not target_trackers:
        tracker_name, tracker_instance = candidates[0]
        log.info(f"Sending test notification using {tracker_name}...")
        success = await tracker_instance.send_test_notification()
        return 0 if success else 1

    overall_success = True
    for tracker_name, tracker_instance in candidates:
        log.info(f"Sending test notification using {tracker_name}...")
        success = await tracker_instance.send_test_notification()
        overall_success = overall_success and success

    return 0 if overall_success else 1


async def main(args: argparse.Namespace):
    """
    Main execution function that initializes trackers and runs the monitoring loop.
    """
    tracker_classes = load_trackers()

    if args.test_notification is not None:
        target_trackers = (
            None if args.test_notification == "__FIRST__" else parse_test_notification_targets(args.test_notification)
        )
        return await run_test_notification(tracker_classes, target_trackers)

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

        # Instantiate API-only trackers that have a configured API key.
        for tracker_name, tracker_class in tracker_classes.items():
            if not getattr(tracker_class, "api_only", False):
                continue
            if not api_tokens.get(tracker_name):
                continue

            tracker_instance = tracker_class(Path("./cookies") / f"{tracker_name}.txt")

            async def wrapped_api_task(t_name: str = tracker_name, inst: Any = tracker_instance) -> tuple[str, Any]:
                try:
                    res = await inst.fetch_notifications()
                    return t_name, res
                except Exception as e:
                    log.error(f"{t_name}: Tracker execution failed.")
                    log.debug(f"{t_name}: Error details: {e}", exc_info=True)
                    return t_name, None

            tasks.append(wrapped_api_task())  # type: ignore

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

                progress.update(
                    main_task,
                    description=f"[bold blue]Processing {tracker_name} [[cyan]{completed_count:02d}/{total_tasks:02d}[/cyan]]",
                    advance=1,
                )

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
        args = parse_args()
        raise SystemExit(asyncio.run(main(args)))
    except KeyboardInterrupt:
        log.info("PTNotifier stopped by user.")
