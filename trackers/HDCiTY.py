#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup
from rich.console import Console

from .base import BaseTracker

console = Console()


class HDCiTY(BaseTracker):
    """
    Manages a session for HDCiTY.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="HDCiTY",
            base_url="https://hdcity.city/",
        )
        self.inbox_url = urljoin(self.base_url, "messages")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from HDCiTY inbox."""
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the inbox for HDCiTY messages."""
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", sucess_text="messages?action=editmailboxes")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        rows = soup.find_all("tr")

        for row in rows:
            checkbox = row.find("input", {"name": "messages[]"})
            if not checkbox:
                continue

            item_id = checkbox.get("value")
            if not item_id or item_id in self.state["processed_ids"]:
                continue

            subject_tag = row.find("a", class_="altlink")
            subject = subject_tag.get_text(strip=True) if subject_tag else "No Subject"

            body_tag = row.find("div", style=lambda x: bool(x and "border:#89a 1px dashed;margin:6px;padding:6px;" in x))
            body = body_tag.get_text(strip=True) if body_tag else ""

            cells = row.find_all("td", class_="rowfollow")
            sender = "System"
            date_str = "Unknown"

            if len(cells) >= 4:
                sender = cells[2].get_text(strip=True)
                date_str = cells[3].get_text(strip=True)

            link = urljoin(self.base_url, f"messages?action=viewmessage&id={item_id}")

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "subject": subject,
                    "body": body,
                    "date": date_str,
                    "url": link,
                    "is_staff": False,
                }
            )

        return new_items
