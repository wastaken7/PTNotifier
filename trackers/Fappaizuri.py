#!/usr/bin/env python3

import re
from pathlib import Path
from typing import Any, override
from urllib.parse import urljoin

from bs4 import BeautifulSoup, Tag

from .base import BaseTracker


class Fappaizuri(BaseTracker):
    """
    Manages a session for Fappaizuri.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="Fappaizuri",
            base_url="https://fappaizuri.me/",
        )
        self.inbox_url = urljoin(self.base_url, "mailbox.php?inbox")

    @override
    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from Fappaizuri mailbox.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        target_url = self.inbox_url
        return await self._parse_messages(target_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="table_mb")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        message_rows = soup.find_all("tr")

        valid_rows: list[Tag] = [
            row for row in message_rows if row and row.find("input", attrs={"name": re.compile(r"msgs\[\d+\]")})
        ]

        for header_row in valid_rows:
            checkbox = header_row.find("input", attrs={"name": re.compile(r"msgs\[(\d+)\]")})
            if not isinstance(checkbox, Tag):
                continue

            checkbox_name = checkbox.get("name")
            if not isinstance(checkbox_name, str):
                continue

            match = re.search(r"(\d+)", checkbox_name)
            if not match:
                continue
            item_id = match.group(1)

            if item_id in self.state["processed_ids"]:
                continue

            cols = header_row.find_all(["th", "td"])
            if len(cols) < 5:
                continue

            sender_cell = cols[2]
            subject_cell = cols[3]
            date_cell = cols[4]

            if not sender_cell or not subject_cell or not date_cell:
                continue

            sender = sender_cell.get_text(strip=True)

            subject_links = subject_cell.find_all("a")
            subject = "No Subject"
            if subject_links:
                last_link = subject_links[-1]
                if last_link:
                    subject = last_link.get_text(strip=True)

            date_str = date_cell.get_text(strip=True)

            body_row = soup.find("tr", id=f"msg_{item_id}")
            body = ""
            if isinstance(body_row, Tag):
                body_div = body_row.find("div", id="mpbox")
                if isinstance(body_div, Tag):
                    body = body_div

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "subject": subject,
                    "body": body,
                    "date": date_str,
                    "url": url,
                    "is_staff": sender.lower() == "system",
                }
            )

        return new_items
