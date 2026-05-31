#!/usr/bin/env python3

from pathlib import Path
from typing import Any, override
from urllib.parse import urljoin

from bs4 import BeautifulSoup
from rich.console import Console

from .base import BaseTracker

console = Console()


class YGGReborn(BaseTracker):
    """
    Manages a session for YGG Reborn.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="YGGReborn",
            base_url="https://www.yggreborn.org/",
        )
        self.inbox_url = urljoin(self.base_url, "inbox/")

    @override
    async def _fetch_items(self) -> list[dict[str, Any]]:
        """
        Fetch unread messages from the inbox.

        Returns:
            list[dict[str, Any]]: List of items.
        """
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(self.inbox_url, "inbox", success_text="Mon compte")
        if not response:
            return new_items

        soup = BeautifulSoup(response, "html.parser")
        rows = soup.find_all("tr", class_="row-hover")

        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 3:
                continue

            unread_indicator = cols[0].find("span", title="Non lu")
            if not unread_indicator:
                continue

            subject_cell = cols[2].find("a")
            if not subject_cell:
                continue

            subject = subject_cell.get_text(strip=True)
            href = str(subject_cell.get("href", ""))
            link = urljoin(self.base_url, href)
            message_id = href.split("/")[-1] if "/" in href else href

            if not message_id or message_id in self.state["processed_ids"]:
                continue

            message = await self._fetch_body(link, subject, message_id)
            if message:
                new_items.append(message)

        return new_items

    async def _fetch_body(self, url: str, subject: str, message_id: str) -> dict[str, Any] | None:
        """
        Fetches the message page and extracts the content.

        Args:
            url (str): The URL to fetch.
            subject (str): The subject of the message.
            message_id (str): The ID of the message.

        Returns:
            dict[str, Any] | None: The message data, or None if not found.
        """
        response = await self._fetch_page(url, "message body")
        if not response:
            return None

        soup = BeautifulSoup(response, "html.parser")

        meta_p = soup.find(
            "p",
            class_="text-xs",
            style=lambda x: bool(x and "var(--text-dark-light)" in x),
        )
        sender = "System"
        date_str = "Unknown"

        if meta_p:
            strong = meta_p.find("strong")
            if strong:
                sender = strong.get_text(strip=True)

            text = meta_p.get_text(strip=True)
            if "·" in text:
                date_str = text.split("·")[-1].strip()

        body_div = soup.find("div", class_="whitespace-pre-wrap")
        body_text = body_div if body_div else "No content"

        return {
            "type": "message",
            "id": message_id,
            "sender": sender,
            "subject": subject,
            "body": body_text,
            "date": date_str,
            "url": url,
        }
