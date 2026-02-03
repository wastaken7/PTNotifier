#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class BrasilTracker(BaseTracker):
    """
    Manages a session for Brasil Tracker using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "BrasilTracker", "https://brasiltracker.org/")
        self.inbox_url = urljoin(self.base_url, "inbox.php")
        self.staff_url = urljoin(self.base_url, "staffpm.php")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch standard and staff messages from Brasil Tracker."""
        inbox_items = await self._parse_messages(self.inbox_url, is_staff=False)
        staff_items = await self._parse_messages(self.staff_url, is_staff=True)
        return inbox_items + staff_items

    async def _parse_messages(self, url: str, is_staff: bool) -> list[dict[str, Any]]:
        """Parses message tables for Brasil Tracker structure."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url, "messages")
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
