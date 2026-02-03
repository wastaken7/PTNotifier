#!/usr/bin/env python3

import re
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class TorrentLeech(BaseTracker):
    """
    Manages a session for TorrentLeech.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            "TorrentLeech",
            "https://www.torrentleech.org/"
        )

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch notifications from TorrentLeech profile."""
        if not self.state.get("notifications_url"):
            soup = await self._fetch_page(self.base_url, "user ID")
            if soup:
                profile_link = soup.find("span", class_="link", onclick=lambda x: x and "/profile/" in x)
                if profile_link:
                    onclick_attr = str(profile_link.get("onclick", ""))
                    match = re.search(r"/profile/([^/]+)/", onclick_attr)
                    if match:
                        username = match.group(1)
                        notif_path = f"profile/{username}/notifications"
                        self.state["notifications_url"] = urljoin(self.base_url, notif_path)

        target_url = self.state.get("notifications_url")
        if not target_url:
            console.print(f"{self.tracker}: [bold red]Initialization failed (Username not found).[/bold red]")
            return []

        return await self._parse_notifications(target_url)

    async def _parse_notifications(self, url: str) -> list[dict[str, Any]]:
        """Parses the notifications table for TorrentLeech."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url, "notifications")

        if not soup:
            return new_items

        table = soup.find("table", id="notificationsTable")
        if not table:
            return new_items

        tbody = table.find("tbody")
        if not tbody:
            return new_items

        rows = tbody.find_all("tr")

        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 4:
                continue

            sent_cell = cols[1]
            item_id = sent_cell.get("data-sort")
            if not item_id:
                continue

            item_id = str(item_id)

            if item_id in self.state["processed_ids"]:
                continue

            date_str = sent_cell.get_text(strip=True)
            msg_cell = cols[3]

            message_text = msg_cell.get_text(" ", strip=True)

            link_element = msg_cell.find("a")
            link = urljoin(self.base_url, str(link_element.get("href", ""))) if link_element else url

            new_items.append(
                {
                    "type": "notification",
                    "id": item_id,
                    "title": "System",
                    "msg": message_text,
                    "date": date_str,
                    "url": link,
                    "is_staff": False,
                }
            )

        return new_items
