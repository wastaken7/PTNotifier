#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup
from rich.console import Console

from .base import BaseTracker

console = Console()


class SceneTime(BaseTracker):
    """
    Manages a session for SceneTime.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="SceneTime",
            base_url="https://www.scenetime.com/",
        )
        self.inbox_url = urljoin(self.base_url, "inbox.php")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from SceneTime inbox."""
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the inbox for SceneTime messages."""
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        message_rows = soup.find_all("div", class_="view_mess")

        for row in message_rows:
            item_id = row.get("rel")
            if not item_id:
                continue

            item_id = str(item_id)

            if item_id in self.state["processed_ids"]:
                continue

            subject_cell = row.find("li", class_="status_icon")
            if not subject_cell:
                continue

            subject = subject_cell.get_text(separator="|", strip=True).split("|")[0]

            date_span = row.find("span", class_="elapsedDate")
            date_str = date_span.get("title") if date_span else "Unknown"

            sender_cell = row.find("li", class_="name")
            sender = sender_cell.get_text(strip=True) if sender_cell else "System"

            link = urljoin(self.base_url, f"inbox.php?id={item_id}")

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "subject": subject,
                    "date": date_str,
                    "url": link,
                    "is_staff": False,
                }
            )

        return new_items
