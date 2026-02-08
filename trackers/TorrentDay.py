#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class TorrentDay(BaseTracker):
    """
    Manages a session for TorrentDay.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="TorrentDay",
            base_url="https://www.torrentday.com/",
        )
        self.inbox_url = "https://www.torrentday.com/m"

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from TorrentDay inbox."""
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the inbox for TorrentDay messages."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url, "messages")

        if not soup:
            return new_items

        inbox_table = soup.find("table", class_="fw t1")
        if not inbox_table:
            return new_items

        rows = inbox_table.find_all("tr")[1:]

        for row in rows:
            cells = row.find_all("td")
            if len(cells) < 3:
                continue

            subject_link = cells[1].find("a")
            if not subject_link:
                continue

            subject = subject_link.get_text(strip=True)
            relative_url = subject_link.get("href")

            if not isinstance(relative_url, str):
                continue

            item_id = relative_url.split("/")[-1].split("#")[0]

            if item_id in self.state["processed_ids"]:
                continue

            date_str = cells[2].get_text(strip=True)
            link = urljoin(self.base_url, relative_url)

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": "Unknown",
                    "subject": subject,
                    "date": date_str,
                    "url": link,
                    "is_staff": False,
                }
            )

        return new_items
