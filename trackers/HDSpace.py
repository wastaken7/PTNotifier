#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup

from utils.console import log

from .base import BaseTracker


class HDSpace(BaseTracker):
    """
    Manages a session for HD-Space.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="HDSpace",
            base_url="https://hd-space.org/",
        )

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from HD-Space mailbox."""
        if not self.state.get("notifications_url"):
            response = await self._fetch_page(self.base_url, "user ID")
            soup = BeautifulSoup(response, "html.parser")

            if soup:
                user_cp_link = soup.find("a", href=lambda h: bool(h and "page=usercp&uid=" in h))
                if user_cp_link:
                    href = str(user_cp_link.get("href", ""))
                    uid = href.split("uid=")[-1].split("&")[0]

                    inbox_path = f"index.php?page=usercp&uid={uid}&do=pm&action=list&what=inbox"
                    self.state["notifications_url"] = urljoin(self.base_url, inbox_path)

        target_url = self.state.get("notifications_url")
        if not target_url:
            log.error(f"{self.tracker}: Initialization failed.")
            return []

        return await self._parse_messages(target_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the XBTIT style message table for HD-Space and fetches their bodies."""
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="Favourites")
        soup = BeautifulSoup(response, "html.parser")

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

            body = await self._fetch_body(link)

            new_items.append(
                {
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
        """Navigates to the message URL and extracts the content body."""
        try:
            response = await self._fetch_page(url, "message body")
            soup = BeautifulSoup(response, "html.parser")

            headers = soup.find_all("td", class_="header")

            body_content = ""
            for header in headers:
                if header.get_text() and "Subject:" in header.get_text():
                    parent_row = header.find_parent("tr")
                    if parent_row:
                        next_row = parent_row.find_next_sibling("tr")
                        if next_row:
                            body_cell = next_row.find("td", class_="lista")
                            if body_cell:
                                body_content = body_cell.get_text(separator="\n\n", strip=True)
                                break

            return body_content
        except Exception as e:
            log.error(f"{self.tracker}: Failed to fetch body for {url}: {e}")
            log.debug("Network error details", exc_info=True)
            return ""
