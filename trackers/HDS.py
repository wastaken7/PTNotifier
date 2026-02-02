#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class HDS(BaseTracker):
    """
    Manages a session for HD-Space using specific cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "HD-Space", "https://hd-space.org/")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from HD-Space mailbox."""
        if not self.state.get("notifications_url"):
            soup = await self._fetch_page(self.base_url)
            if soup:
                user_cp_link = soup.find("a", href=lambda h: h and "page=usercp&uid=" in h)
                if user_cp_link:
                    href = str(user_cp_link.get("href", ""))
                    uid = href.split("uid=")[-1].split("&")[0]

                    inbox_path = f"index.php?page=usercp&uid={uid}&do=pm&action=list&what=inbox"
                    self.state["notifications_url"] = urljoin(self.base_url, inbox_path)

        target_url = self.state.get("notifications_url")
        if not target_url:
            console.print(f"{self.tracker}: [bold red]Initialization failed.[/bold red]")
            return []

        return await self._parse_messages(target_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the XBTIT style message table for HD-Space."""
        new_items: list[dict[str, Any]] = []
        soup = await self._fetch_page(url)

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
