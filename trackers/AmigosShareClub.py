#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class AmigosShareClub(BaseTracker):
    """
    Manages a session for Amigos Share Club (ASC).
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            "AmigosShareClub",
            "https://cliente.amigos-share.club/",
        )
        self.inbox_url = "https://cliente.amigos-share.club/mensagens.php?do=entrada"

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from ASC inbox."""
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the inbox for ASC messages."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url, "messages")

        if not soup:
            return new_items

        inbox_table = soup.find("table", class_="table-inbox")
        if not inbox_table:
            return new_items

        message_rows = inbox_table.find_all("tr")

        for row in message_rows:
            item_id = row.get("id")
            if not item_id:
                continue

            item_id = str(item_id)

            if item_id in self.state["processed_ids"]:
                continue

            sender_cell = row.find("td", class_="inbox-small-cells")
            sender = "System"
            if sender_cell:
                nick_tag = sender_cell.find("span", class_="nick")
                if nick_tag:
                    sender = nick_tag.get_text(strip=True)

            subject_cell = row.find("td", class_="view-message")
            subject = "No Subject"
            if subject_cell:
                subject_link = subject_cell.find("a")
                if subject_link:
                    subject = subject_link.get_text(strip=True)

            date_cell = row.find_all("td", class_="view-message")
            date_str = "Unknown"
            if len(date_cell) >= 2:
                date_str = date_cell[1].get_text(strip=True)

            link = urljoin(self.base_url, f"mensagens.php?action=ver_mp&id_mp={item_id}")

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "msg": subject,
                    "date": date_str,
                    "url": link,
                    "is_staff": False,
                }
            )

        return new_items
