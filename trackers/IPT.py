#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class IPT(BaseTracker):
    """
    Manages a session for IPTorrents using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "IPTorrents", "https://iptorrents.com/")
        self.inbox_url = urljoin(self.base_url, "inbox")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from IPTorrents inbox."""
        return await self._parse_messages(self.inbox_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the message list structure from IPTorrents HTML."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url)

        if not soup:
            return new_items

        message_list = soup.find("ol", class_="list")
        if not message_list:
            return new_items

        rows = message_list.find_all("li", class_="cRow")

        for row in rows:
            raw_id = row.get("id", "")
            raw_id = raw_id if isinstance(raw_id, str) else ""
            item_id = raw_id.replace("c", "") if raw_id.startswith("c") else raw_id

            if not item_id or item_id in self.state["processed_ids"]:
                continue

            author_span = row.find("span", class_="t")
            sender = author_span.get_text(strip=True) if author_span else "System"
            sender = sender.replace("Staff Message", "")

            is_staff = False
            if author_span and author_span.find("i", class_="fa-warning"):
                is_staff = True

            sub_div = row.find("div", class_="sub")
            subject = sub_div.get_text(strip=True) if sub_div else "No Subject"

            date_el = row.find("span", class_="elapsedDate")
            date_str = date_el.get("title", "") if date_el else "Unknown"

            link = urljoin(self.base_url, "inbox")

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
