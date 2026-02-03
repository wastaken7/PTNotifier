#!/usr/bin/env python3

import re
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class HDTorrents(BaseTracker):
    """
    Manages a session for HD-Torrents using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            "HD-Torrents",
            "https://hd-torrents.org/",
            custom_headers={
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            }
        )

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from HD-Torrents mailbox."""
        if not self.state.get("notifications_url"):
            soup = await self._fetch_page(self.base_url, "user ID")
            if soup:
                user_cp_link = soup.find("a", href=lambda h: h and "usercp.php?uid=" in h)
                if user_cp_link:
                    href = str(user_cp_link.get("href", ""))
                    match = re.search(r'uid=(\d+)', href)
                    if match:
                        uid = match.group(1)
                        print(uid)
                        inbox_path = f"usercp.php?uid={uid}&do=pm&action=list"
                        self.state["notifications_url"] = urljoin(self.base_url, inbox_path)

        target_url = self.state.get("notifications_url")
        if not target_url:
            console.print(f"{self.tracker}: [bold red]Initialization failed (UID not found).[/bold red]")
            return []

        return await self._parse_messages(target_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the message table for HD-Torrents."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url, "messages")

        form = soup.find("form", attrs={"name": "deleteall"}) if soup else None
        if not form:
            return new_items

        table = form.find("table", class_="lista")
        if not table:
            return new_items

        rows = table.find_all("tr")[1:]

        for row in rows:
            cols = row.find_all("td", class_="lista")
            if len(cols) < 4:
                continue

            subject_cell = cols[3].find("a")
            if not subject_cell:
                continue

            subject = subject_cell.get_text(strip=True)
            href = subject_cell.get("href", "")
            link = urljoin(self.base_url, str(href))

            item_id = link.split("id=")[-1].split("&")[0] if "id=" in link else link

            if item_id in self.state["processed_ids"]:
                continue

            sender = cols[1].get_text(strip=True)
            date_str = cols[2].get_text(strip=True).replace("\xa0", " ")

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "msg": subject,
                    "date": date_str,
                    "url": link,
                    "is_staff": False,
                }
            )

        return new_items
