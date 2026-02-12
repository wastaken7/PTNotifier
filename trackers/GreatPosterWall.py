#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup
from rich.console import Console

from .base import BaseTracker

console = Console()


class GreatPosterWall(BaseTracker):
    """
    Manages a session for GreatPosterWall.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="GreatPosterWall",
            base_url="https://greatposterwall.com/",
        )
        self.inbox_url = urljoin(self.base_url, "inbox.php?sort=unread")
        self.staff_url = urljoin(self.base_url, "staffpm.php")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch standard and staff messages."""
        inbox_items = await self._parse_messages(self.inbox_url, is_staff=False)
        staff_items = await self._parse_messages(self.staff_url, is_staff=True)
        return inbox_items + staff_items

    async def _parse_messages(self, url: str, is_staff: bool) -> list[dict[str, Any]]:
        """Parses the message table and fetches bodies for unread conversations."""
        new_items: list[dict[str, Any]] = []
        message_type = "messages" if not is_staff else "staff messages"
        response = await self._fetch_page(url, message_type, sucess_text="forums.php")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return new_items

        tables = soup.find_all("table", class_=lambda x: bool(x and ("TableUserInbox" in x or "Table" in x)))
        if not tables:
            return new_items

        for table in tables:
            rows = table.find_all("tr", class_="Table-row")
            for row in rows:
                cols = row.find_all("td")
                if len(cols) < 3:
                    continue

                subject_cell = cols[1].find("a")
                if not subject_cell:
                    continue

                is_unread = bool(cols[1].find("strong"))
                if not is_unread:
                    continue

                subject = subject_cell.get_text(strip=True)
                href = subject_cell.get("href", "")
                link = urljoin(self.base_url, str(href))

                messages = await self._fetch_body(link, subject, is_staff)
                new_items.extend(messages)

        return new_items

    async def _fetch_body(self, url: str, subject: str, is_staff: bool) -> list[dict[str, Any]]:
        """
        Fetches the conversation page and extracts individual messages.
        """
        messages_found: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "message body")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return messages_found

        containers = soup.find_all("div", class_="Box")

        for box in containers:
            body_div = box.find("div", id=lambda x: bool(x and x.startswith("message")))
            if not body_div:
                continue

            raw_id = body_div.get("id", "")
            message_id = str(raw_id).replace("message", "") if raw_id else None

            if not message_id or message_id in self.state["processed_ids"]:
                continue

            header = box.find("div", class_="Box-header")
            sender = "System"
            date_str = "Unknown"

            if header:
                sender_link = header.find("a", href=lambda x: bool(x and "user.php?id=" in x))
                if sender_link:
                    sender = sender_link.get_text(strip=True)

                date_span = header.find("span", class_="tooltipstered")
                if date_span:
                    date_str = date_span.get_text(strip=True)

            body_text = body_div.get_text("\n\n", strip=True)

            messages_found.append(
                {
                    "type": "message",
                    "id": message_id,
                    "sender": sender,
                    "subject": subject,
                    "body": body_text,
                    "date": date_str,
                    "url": f"{url}#message{message_id}",
                    "is_staff": is_staff,
                }
            )

        return messages_found
