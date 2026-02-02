#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class FL(BaseTracker):
    """
    Manages a session for FileList using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "FileList", "https://filelist.io/")
        self.inbox_url = urljoin(self.base_url, "messages.php")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from FileList inbox."""
        return await self._parse_messages(self.inbox_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses message structure for FileList."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url)

        if not soup:
            return new_items

        message_links = soup.find_all("a", href=lambda x: x and "action=viewmessage" in x)
        # link_tag: Tag
        for link_tag in message_links:
            subject_div = link_tag.find("div", class_="normalline")
            if not subject_div:
                continue

            subject = subject_div.get_text(strip=True)
            href = link_tag.get("href", "")
            link = urljoin(self.base_url, str(href))
            item_id = link.split("id=")[-1] if "id=" in link else link

            if item_id in self.state["processed_ids"]:
                continue

            sender_div = link_tag.find_previous_sibling("div", class_="normalline")
            date_div = link_tag.find_next_sibling("div", class_="normalline")

            sender = sender_div.get_text(strip=True) if sender_div else "System"
            date_str = date_div.get_text(strip=True) if date_div else "Unknown"

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
