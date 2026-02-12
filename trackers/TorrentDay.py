#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup

from .base import BaseTracker, log


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

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from TorrentDay inbox."""
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the inbox for TorrentDay messages and filters unread ones."""
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages")
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
            if not status_img:
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
        """Navigates to the conversation and extracts the last message body."""
        try:
            response = await self._fetch_page(url, "message body")
            soup = BeautifulSoup(response, "html.parser")
            message_containers = soup.find_all("div", class_="postContainer")
            if message_containers:
                last_container = message_containers[-1]
                body_div = last_container.find("div", class_="postContents")
                if body_div:
                    return body_div.get_text(separator="\n\n", strip=True)

            return ""
        except Exception:
            log.error(f"{self.tracker}: Failed to fetch body for {url}:", exc_info=True)
            return ""
