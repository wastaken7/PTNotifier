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


class GPW(BaseTracker):
    """
    Manages a session for GPW tracker using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "GPW", "https://greatposterwall.com/")
        self.inbox_url = urljoin(self.base_url, "inbox.php")
        self.staff_url = urljoin(self.base_url, "staffpm.php")

    async def process(
        self,
        send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]],
    ):
        """Main loop to fetch standard and staff messages."""
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
        """Parses all message tables for both Inbox and Staff PMs."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url)
        if not soup:
            return new_items

        tables = soup.find_all("table", class_=lambda x: x and ("TableUserInbox" in x or "Table" in x))
        if not tables:
            return new_items

        for table in tables:
            rows = table.find_all("tr", class_="Table-row")
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
                    date_str = cols[3].get_text(strip=True) if len(cols) > 3 else cols[2].get_text(strip=True)
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
        """Module-level entry point for GPW."""
        cookie_files = glob.glob(str(Path("./cookies") / "OTHER" / "GPW.txt"))
        if not cookie_files:
            return

        sessions = [GPW(Path(f)) for f in cookie_files]
        tasks = [session.process(send_telegram) for session in sessions]
        await asyncio.gather(*tasks)
