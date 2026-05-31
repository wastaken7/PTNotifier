#!/usr/bin/env python3

from pathlib import Path
from typing import Any, override
from urllib.parse import urljoin

from bs4 import BeautifulSoup

from utils.console import log

from .base import BaseTracker


class TorrentDay(BaseTracker):
    """
    Manages a session for TorrentDay.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="TorrentDay",
            base_url="https://www.torrentday.com/",
        )
        self.inbox_url = "https://www.torrentday.com/m"

    @override
    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from TorrentDay inbox.

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
        """Parses the inbox for TorrentDay messages and filters unread ones.

        Args:
            url (str): The URL to parse.
            include_read (bool): Whether to include read messages.
            ignore_processed (bool): Whether to ignore processed messages.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="mybonus.php")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        inbox_table = soup.find("table", class_="fw t1")
        if not inbox_table:
            return new_items

        rows = inbox_table.find_all("tr")[1:]

        for row in rows:
            cells = row.find_all("td")
            if len(cells) < 3:
                continue

            status_img = cells[0].find("img", src=lambda s: bool(s and "unreadMsg.png" in s))
            if not status_img and not include_read:
                continue

            sender_link = cells[0].find("a")
            sender = sender_link.get_text(strip=True) if sender_link else "System"

            subject_link = cells[1].find("a")
            if not subject_link:
                continue

            subject = subject_link.get_text(strip=True)
            relative_url = subject_link.get("href")
            if not isinstance(relative_url, str):
                continue

            item_id = relative_url.split("/")[-1].split("#")[0]
            if not ignore_processed and item_id in self.state["processed_ids"]:
                continue

            date_str = cells[2].get_text(strip=True)
            full_link = urljoin(self.base_url, relative_url)

            body = await self._fetch_body(full_link)

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "subject": subject,
                    "body": body,
                    "date": date_str,
                    "url": full_link,
                    "is_staff": sender.lower() == "system",
                }
            )

        return new_items

    async def _fetch_body(self, url: str) -> str:
        """Navigates to the conversation and extracts the last message body.

        Args:
            url (str): The URL to parse.

        Returns:
            str: The body of the message.
        """
        try:
            response = await self._fetch_page(url, "message body")
            soup = BeautifulSoup(response, "html.parser")
            message_containers = soup.find_all("div", class_="postContainer")
            if message_containers:
                last_container = message_containers[-1]
                body_div = last_container.find("div", class_="postContents")
                if body_div:
                    return str(body_div)

            return ""
        except Exception as e:
            log.error(f"{self.tracker}: Failed to fetch body for {url}: {e}")
            log.debug(f"{self.tracker}: Network error details", exc_info=True)
            return ""
