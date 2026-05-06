#!/usr/bin/env python3

import re
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup

from utils.console import log

from .base import BaseTracker


class BTArg(BaseTracker):
    """
    Manages a session for the BTArg tracker.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="BTArg",
            base_url="https://www.btarg.com.ar/tracker/",
        )

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from BTArg mailbox.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        inbox_url = urljoin(self.base_url, "messages.php?action=viewmailbox")
        self.state["notifications_url"] = inbox_url

        return await self._parse_messages(inbox_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the message table for BTArg.

        Args:
            url (str): The URL to parse.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="action=viewmessage")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        form = soup.find("form", attrs={"name": "mensajes"})
        if not form:
            return new_items

        table = form.find("table")
        if not table:
            return new_items

        # BTArg message rows use 'onclick="selclic(...)"'
        rows = table.find_all("tr", onclick=True)

        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 5:
                continue

            # Column 1 (index 1) contains the subject link
            subject_cell = cols[1].find("a", href=lambda h: bool(h and "action=viewmessage" in h))
            if not subject_cell:
                continue

            subject = subject_cell.get_text(strip=True)
            href = subject_cell.get("href", "")
            link = urljoin(self.base_url, str(href))

            # Extracts the message ID from the URL (ex: id=1993382)
            match_id = re.search(r'id=(\d+)', link)
            item_id = match_id.group(1) if match_id else link

            if item_id in self.state["processed_ids"]:
                continue

            # Sender is in column 2, date in column 3
            sender = cols[2].get_text(strip=True)
            date_str = cols[3].get_text(strip=True)

            # Fetch the message body by accessing the individual link
            body = await self._fetch_body(link)

            new_items.append(
                {
                    "favicon": f"{self.base_url}pic/btarg.ico",
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

    async def _fetch_body(self, url: str) -> str:
        """Navigates to the message URL and extracts the body text.

        Args:
            url (str): Individual message URL.

        Returns:
            str: Message text.
        """
        try:
            response = await self._fetch_page(url, "corpo da mensagem")
            soup = BeautifulSoup(response, "html.parser")

            body_div = soup.find("div", style=lambda s: bool(s and "overflow:auto" in s))
            if body_div:
                return body_div.get_text(separator="\n", strip=True)

            return ""
        except Exception as e:
            log.error(f"{self.tracker}: Failed to fetch message body in {url}: {e}")
            return ""
