#!/usr/bin/env python3

import re
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class ImmortalSeed(BaseTracker):
    """
    Manages a session for ImmortalSeed using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "ImmortalSeed", "https://immortalseed.me/")
        self.inbox_url = urljoin(self.base_url, "messages.php")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from ImmortalSeed inbox."""
        return await self._parse_messages(self.inbox_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses message rows for ImmortalSeed structure."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url, "messages")

        if not soup or not soup.find("form", attrs={"name": "messageform"}):
            return new_items

        container = soup.find("tbody", id="collapseobj_messages")
        if not container:
            return new_items

        rows = container.find_all("tr", recursive=False)
        for row in rows:
            cols = row.find_all("td", recursive=False)
            if len(cols) < 2:
                continue

            subject_div = cols[1].find("div")
            if not subject_div:
                continue

            subject_cell = subject_div.find("a")
            if not subject_cell:
                continue

            subject = subject_cell.get_text(strip=True)
            href = subject_cell.get("href", "")
            link = urljoin(self.base_url, str(href))

            pmid_match = re.search(r"pmid=(\d+)", link)
            item_id = pmid_match.group(1) if pmid_match else link

            if item_id in self.state["processed_ids"]:
                continue

            date_span = subject_div.find("span", class_="smallfont")
            date_str = date_span.get_text(strip=True) if date_span else "Unknown"

            sender_div = cols[1].find("div", class_="smalltext")
            sender = "System"
            if sender_div:
                sender_link = sender_div.find("a")
                if sender_link:
                    sender = sender_link.get_text(strip=True)

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
