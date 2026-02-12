#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from bs4 import BeautifulSoup

from .base import BaseTracker, log


class IPTorrents(BaseTracker):
    """
    Manages a session for IPTorrents.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="IPTorrents",
            base_url="https://iptorrents.com/",
        )
        self.inbox_url = urljoin(self.base_url, "inbox")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from IPTorrents inbox."""
        return await self._parse_messages(self.inbox_url)

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the message list structure from IPTorrents HTML."""
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", sucess_text="settings.php")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        message_list = soup.find("ol", class_="list")
        if not message_list:
            return new_items

        rows = message_list.find_all("li", class_="cRow")

        for row in rows:
            raw_id = row.get("id", "")
            raw_id = raw_id if isinstance(raw_id, str) else ""
            item_id = raw_id.replace("c", "") if raw_id.startswith("c") else raw_id

            if not item_id or item_id in self.state["processed_ids"]:
                continue

            author_span = row.find("span", class_="t")
            sender = author_span.get_text(strip=True) if author_span else "System"
            sender = sender.replace("Staff Message", "")

            is_staff = False
            if author_span and author_span.find("i", class_="fa-warning"):
                is_staff = True

            date_el = row.find("span", class_="elapsedDate")
            date_str = date_el.get("title", "") if date_el else "Unknown"

            link = urljoin(self.base_url, "inbox")
            body = await self._fetch_body(item_id)
            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "body": body,
                    "date": date_str,
                    "url": link,
                    "is_staff": is_staff,
                }
            )

        return new_items

    async def _fetch_body(self, item_id: str) -> str:
        """
        Fetches the message body using the IPTorrents XHR API.
        The API returns a JSON containing HTML instructions.
        """
        api_url = urljoin(self.base_url, "API.php")

        payload = {"jxt": "2", "jxw": "c", "i": item_id}

        headers = {
            "Content-Type": "application/x-www-form-urlencoded; charset=UTF-8",
            "X-Requested-With": "XMLHttpRequest",
            "Origin": self.base_url.rstrip("/"),
            "Referer": self.inbox_url,
        }
        try:
            response = await self.client.post(api_url, data=payload, headers=headers)

            data = response.json()

            if "Fs" in data:
                for instruction in data["Fs"]:
                    if instruction[0] == "DOM" and ".msgBody" in str(instruction[1]):
                        cmd: list[Any] = []
                        for cmd in instruction[1]:
                            if cmd and cmd[0] == "html":
                                body_soup = BeautifulSoup(cmd[1], "html.parser")
                                body_el = body_soup.find("blockquote", class_="body")
                                if body_el:
                                    return body_el.get_text(separator="\n\n", strip=True)
            return ""
        except Exception:
            log.error(f"{self.tracker}: API Error for {item_id}:", exc_info=True)
            return ""
