#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urljoin, urlparse

from bs4 import BeautifulSoup

from .base import BaseTracker, log


class PTSKit(BaseTracker):
    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="PTSKit",
            base_url="https://www.ptskit.org/",
        )
        self.inbox_url = urljoin(self.base_url, "messages.php?action=viewmailbox&box=1&unread=yes")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        inbox_items = await self._parse_messages(self.inbox_url)
        return inbox_items

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", sucess_text="viewrequests.php")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        for row in soup.find_all("tr"):
            cells = row.find_all("td", class_="rowfollow")
            if len(cells) < 4:
                continue

            subject_link = cells[1].find("a", href=True)
            if not subject_link or "viewmessage" not in subject_link["href"]:
                continue

            full_link = urljoin(self.base_url, str(subject_link["href"]))
            parsed_url = urlparse(full_link)
            query_params = parse_qs(parsed_url.query)
            item_id = query_params.get("id", [None])[0]

            if not item_id or item_id in self.state["processed_ids"]:
                continue

            subject = subject_link.get_text(strip=True)
            sender = cells[2].get_text(strip=True)

            date_span = cells[3].find("span", title=True)
            date_str = date_span["title"] if date_span else cells[3].get_text(strip=True)
            body = await self._fetch_body(full_link)

            new_items.append(
                {
                    "type": "message",
                    "id": str(item_id),
                    "title": sender,
                    "subject": subject,
                    "body": body,
                    "date": date_str,
                    "url": full_link,
                    "is_staff": False,
                }
            )

        return new_items

    async def _fetch_body(self, url: str) -> str:
        """Navigates to the message URL and extracts the content body."""
        try:
            response = await self._fetch_page(url, "message body")
            soup = BeautifulSoup(response, "html.parser")

            body_td = soup.find("td", attrs={"colspan": "2", "align": "left"})

            if body_td:
                parent_table = body_td.find_parent("table")
                if parent_table and parent_table.get("width") == "737":
                    return body_td.get_text(separator="\n\n", strip=True)

            return ""
        except Exception:
            log.error(f"{self.tracker}: Failed to fetch body for {url}:", exc_info=True)
            return ""
