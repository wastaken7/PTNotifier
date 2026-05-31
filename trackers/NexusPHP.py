#!/usr/bin/env python3
from pathlib import Path
from typing import Any, override
from urllib.parse import urljoin

from bs4 import BeautifulSoup

from utils.console import log

from .base import BaseTracker


class NexusPHP(BaseTracker):
    """
    Generic tracker for NexusPHP sites.
    """

    def __init__(self, cookie_path: Path, tracker_name: str, base_url: str, **kwargs: Any):
        super().__init__(
            cookie_path=cookie_path,
            tracker_name=tracker_name,
            base_url=base_url,
            **kwargs,
        )
        self.inbox_url = urljoin(self.base_url, "messages.php?action=viewmailbox&box=1&unread=yes")

    @override
    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from NexusPHP inbox.

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

        read_items = await self._parse_messages(
            urljoin(self.base_url, "messages.php?action=viewmailbox&box=1"),
            include_read=True,
            ignore_processed=True,
        )
        if read_items:
            return read_items[0]

        return None

    async def _parse_messages(self, url: str, include_read: bool = False, ignore_processed: bool = False) -> list[dict[str, Any]]:
        """Parses message rows for NexusPHP structure.

        Args:
            url (str): The URL to parse.
            include_read (bool): Whether to include read messages.
            ignore_processed (bool): Whether to ignore processed messages.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="torrents.php")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        tables = soup.find_all("table", {"width": "737", "cellpadding": "4"})

        target_table = None
        for table in tables:
            if table.find("td", text=lambda x: bool(x and "Subject" in x)):
                target_table = table
                break

        if not target_table:
            return new_items

        rows = target_table.find_all("tr")

        for row in rows:
            unread_img = row.find("img", class_="unreadpm")
            if not unread_img and not include_read:
                continue

            cells = row.find_all("td", class_="rowfollow")
            if len(cells) < 4:
                continue

            link_tag = cells[1].find("a", href=lambda x: bool(x and "action=viewmessage" in x))
            if not link_tag:
                continue

            msg_url = urljoin(self.base_url, str(link_tag["href"]))
            item_id = msg_url.split("id=")[-1]

            if not ignore_processed and item_id in self.state["processed_ids"]:
                continue

            subject = link_tag.get_text(strip=True)
            sender = cells[2].get_text(strip=True)

            date_span = cells[3].find("span", title=True)
            date_str = date_span["title"] if date_span else cells[3].get_text(strip=True)
            body = await self._fetch_body(msg_url)

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "sender": sender,
                    "subject": subject,
                    "body": body,
                    "date": date_str,
                    "url": msg_url,
                    "is_staff": False,
                }
            )

        return new_items

    async def _fetch_body(self, url: str) -> str:
        """Navigates to the message URL and extracts the content body.

        Args:
            url (str): The URL to parse.

        Returns:
            str: The body of the message.
        """
        try:
            response = await self._fetch_page(url, "message body")
            soup = BeautifulSoup(response, "html.parser")
            all_tds = soup.find_all("td", attrs={"colspan": "2", "align": "left"})

            for td in all_tds:
                parent_table = td.find_parent("table")
                if parent_table and parent_table.get("width") == "737":
                    return str(td)

            return ""
        except Exception as e:
            log.error(f"{self.tracker}: Failed to fetch body for {url}: {e}")
            log.debug(f"{self.tracker}: Network error details", exc_info=True)
            return ""
