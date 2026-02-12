#!/usr/bin/env python3

import copy
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
        """Parses the inbox for SceneTime messages and extracts bodies from hidden divs."""
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", sucess_text="request.php")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        message_headers = soup.find_all("div", class_="view_mess")

        for header in message_headers:
            is_unread = header.get("type") == "unread"

            if not is_unread:
                continue

            item_id = header.get("rel")
            if not item_id:
                continue
            item_id = str(item_id)

            if item_id in self.state["processed_ids"]:
                continue

            status_li = header.find("li", class_="status_icon")
            subject = "No Subject"
            if status_li:
                subject = status_li.get_text(separator="|", strip=True).split("|")[0]

            date_span = header.find("span", class_="elapsedDate")
            date_str = date_span.get("title") if date_span else "Unknown"

            sender_cell = header.find("li", class_="name")
            sender = sender_cell.get_text(strip=True) if sender_cell else "System"

            body = ""
            body_div = soup.find("div", id=f"messa_{item_id}")
            if body_div:
                temp_soup = copy.copy(body_div)

                controls = temp_soup.find("div", style=lambda s: bool(s and "border-top" in s))
                if controls:
                    controls.decompose()

                body = temp_soup.get_text(separator="\n", strip=True)

            link = urljoin(self.base_url, f"inbox.php?id={item_id}")

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "subject": subject,
                    "body": body,
                    "date": date_str,
                    "url": link,
                }
            )

        return new_items
