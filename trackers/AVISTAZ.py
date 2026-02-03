#!/usr/bin/env python3

from pathlib import Path
from typing import Any

from rich.console import Console

from .base import BaseTracker

console = Console()


class AvistaZ(BaseTracker):
    """
    Manages a session for a specific cookie file (site/user).
    """

    def __init__(self, cookie_path: Path):
        self.domain = self._extract_domain_from_cookie(cookie_path)
        super().__init__(cookie_path, self.domain, f"https://{self.domain}/")

        self.notifications_url = self.base_url + "notifications"
        self.messages_url = self.base_url + "messenger"

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch all new items from the tracker."""
        notifications = await self._fetch_and_parse_notifications()
        messages = await self._fetch_and_parse_messages()
        return notifications + messages

    async def _fetch_and_parse_notifications(self) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(self.notifications_url, "notifications")
        if not soup:
            return new_items

        table = soup.find("table", class_="table-hover")
        if not table:
            return new_items

        tbody = table.find("tbody")
        if not tbody:
            return new_items

        rows = tbody.find_all("tr")
        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 3:
                continue

            date_posted = cols[0].get_text(strip=True)
            title_cell = cols[1].find("a")
            title = title_cell.get_text(strip=True) if title_cell else cols[1].get_text(strip=True)
            link = title_cell["href"] if title_cell else self.notifications_url
            notification_msg = cols[2].get_text(strip=True)

            if link in self.state["processed_ids"]:
                continue

            new_items.append(
                {
                    "type": "notification",
                    "id": link,
                    "title": title,
                    "msg": notification_msg,
                    "date": date_posted,
                    "url": link,
                }
            )
        return new_items

    async def _fetch_and_parse_messages(self) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(self.messages_url, "messages")
        if not soup:
            return new_items

        table = soup.find("table", class_="table-hover")
        if not table:
            return new_items

        tbody = table.find("tbody")
        if not tbody:
            return new_items

        rows = tbody.find_all("tr")
        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 5:
                continue

            sender = cols[2].get_text(strip=True)
            subject_cell = cols[3].find("a")
            if not subject_cell:
                continue

            subject = subject_cell.get_text(strip=True)
            link = subject_cell["href"]
            age = cols[4].get_text(strip=True)

            if link in self.state["processed_ids"]:
                continue

            new_items.append(
                {
                    "type": "message",
                    "id": link,
                    "title": f"PM from {sender}",
                    "msg": f"Subject: {subject}",
                    "date": age,
                    "url": link,
                }
            )
        return new_items
