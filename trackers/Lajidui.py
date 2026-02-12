#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup
from rich.console import Console

from .base import BaseTracker

console = Console()


class Lajidui(BaseTracker):
    """
    Manages a session for Lajidui.
    """
    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="Lajidui",
            base_url="https://pt.lajidui.top/",
        )
        self.inbox_url = urljoin(self.base_url, "messages.php?action=viewmailbox&box=1&unread=yes")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages")
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
            if not unread_img:
                continue

            cells = row.find_all("td", class_="rowfollow")
            if len(cells) < 4:
                continue

            link_tag = cells[1].find("a", href=lambda x: bool(x and "action=viewmessage" in x))
            if not link_tag:
                continue

            msg_url = urljoin(self.base_url, str(link_tag["href"]))
            item_id = msg_url.split("id=")[-1]

            if item_id in self.state["processed_ids"]:
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
                    "title": sender,
                    "subject": subject,
                    "body": body,
                    "date": date_str,
                    "url": msg_url,
                    "is_staff": False,
                }
            )

        return new_items

    async def _fetch_body(self, url: str) -> str:
        """Navigates to the message URL and extracts the content body."""
        try:
            response = await self._fetch_page(url, "message body")
            soup = BeautifulSoup(response, "html.parser")
            all_tds = soup.find_all("td", attrs={"colspan": "2", "align": "left"})

            for td in all_tds:
                parent_table = td.find_parent("table")
                if parent_table and parent_table.get("width") == "737":
                    return td.get_text(separator="\n\n", strip=True)

            return ""
        except Exception as e:
            console.print(f"{self.tracker}: [bold red]Failed to fetch body for {url}: {e}[/bold red]")
            return ""
