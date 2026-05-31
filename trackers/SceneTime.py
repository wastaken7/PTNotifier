#!/usr/bin/env python3

import copy
from pathlib import Path
from typing import Any, override
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

    @override
    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from SceneTime inbox.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    @override
    async def _fetch_test_item(self) -> dict[str, Any] | None:
        """Fetch a test item from the tracker.

        Returns:
            dict[str, Any] | None: Test item or None if not found.
        """
        unread_items = await self._fetch_items()
        if unread_items:
            return unread_items[0]

        read_items = await self._parse_messages(self.inbox_url, include_read=True, ignore_processed=True)
        if read_items:
            return read_items[0]

        return None

    async def _parse_messages(self, url: str, include_read: bool = False, ignore_processed: bool = False) -> list[dict[str, Any]]:
        """Parses the inbox for SceneTime messages and extracts bodies from hidden divs.

        Args:
            url (str): The URL to parse.
            include_read (bool): Whether to include read messages.
            ignore_processed (bool): Whether to ignore processed messages.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="request.php")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        message_headers = soup.find_all("div", class_="view_mess")

        for header in message_headers:
            is_unread = header.get("type") == "unread"

            if not is_unread and not include_read:
                continue

            item_id = header.get("rel")
            if not item_id:
                continue
            item_id = str(item_id)

            if not ignore_processed and item_id in self.state["processed_ids"]:
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

                body = temp_soup

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
