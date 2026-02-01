#!/usr/bin/env python3

import asyncio
import glob
from collections.abc import Coroutine
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class BT(BaseTracker):
    """
    Manages a session for Brasil Tracker using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "BT", "https://brasiltracker.org/")
        self.inbox_url = urljoin(self.base_url, "inbox.php")
        self.staff_url = urljoin(self.base_url, "staffpm.php")

    async def process(
        self,
        send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]],
    ):
        """Main loop to fetch standard and staff messages from Brasil Tracker."""
        try:
            inbox_items = await self._fetch_messages(self.inbox_url, is_staff=False)
            staff_items = await self._fetch_messages(self.staff_url, is_staff=True)

            all_items = inbox_items + staff_items

            for item in all_items:
                if not self.first_run:
                    await send_telegram(item, self.base_url, item["url"])
                    await asyncio.sleep(3)
                self._ack_item(item["id"])

        except Exception as e:
            console.print(f"{self.tracker}: Error processing {self.base_url}: {e}")
        finally:
            await self.client.aclose()

    async def _fetch_messages(self, url: str, is_staff: bool) -> list[dict[str, Any]]:
        """Parses message tables for Brasil Tracker structure."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url)
        if not soup or not soup.find(id="messageform"):
            return new_items

        tables = soup.find_all("table", class_="message_table")
        if not tables:
            return new_items

        for table in tables:
            rows = table.find_all("tr", class_=["rowa", "rowb"])
            for row in rows:
                cols = row.find_all("td")
                if len(cols) < 3:
                    continue

                subject_cell = cols[1].find("a")
                if not subject_cell:
                    continue

                subject = subject_cell.get_text(strip=True)
                href = subject_cell.get("href", "")
                link = urljoin(self.base_url, str(href))
                item_id = link.split("id=")[-1] if "id=" in link else link

                if item_id in self.state["processed_ids"]:
                    continue

                if is_staff:
                    sender = "Staff System"
                    date_str = cols[2].get_text(strip=True)
                else:
                    sender = cols[2].get_text(strip=True) if len(cols) > 2 else "System"
                    date_str = cols[3].get_text(strip=True) if len(cols) > 3 else "Unknown"

                new_items.append(
                    {
                        "type": "message",
                        "id": item_id,
                        "title": sender,
                        "msg": subject,
                        "date": date_str,
                        "url": link,
                        "is_staff": is_staff,
                    }
                )
        return new_items

    @staticmethod
    async def fetch_notifications(send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]]):
        """Module-level entry point for Brasil Tracker."""
        cookie_files = glob.glob(str(Path("./cookies") / "OTHER" / "BT.txt"))
        if not cookie_files:
            return

        sessions = [BT(Path(f)) for f in cookie_files]
        tasks = [session.process(send_telegram) for session in sessions]
        await asyncio.gather(*tasks)
