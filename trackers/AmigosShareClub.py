#!/usr/bin/env python3

from pathlib import Path
from typing import Any
from urllib.parse import urljoin

import httpx
from bs4 import BeautifulSoup

from utils.console import log

from .base import BaseTracker


class AmigosShareClub(BaseTracker):
    """
    Manages a session for Amigos Share Club (ASC).
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="AmigosShareClub",
            base_url="https://cliente.amigos-share.club/",
        )
        self.inbox_url = "https://cliente.amigos-share.club/mensagens.php?do=n_lidas"

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from ASC inbox and include their bodies."""
        inbox_items: list[dict[str, Any]] = await self._parse_messages(self.inbox_url)

        for item in inbox_items:
            item["body"] = await self._fetch_body(item["id"])

        return inbox_items

    async def _fetch_body(self, message_id: str) -> str:
        """
        Fetches the message body using the internal search API (JSON response)
        """
        payload = {"mp": message_id, "type": "entrada"}

        headers = {
            "Content-Type": "application/x-www-form-urlencoded; charset=UTF-8",
            "X-Requested-With": "XMLHttpRequest",
            "User-Agent": "PTNotifier 1.0 (https://github.com/wastaken7/PTNotifier)",
        }

        try:
            response = await self.client.post(
                "https://cliente.amigos-share.club/search.php",
                data=payload,
                headers=headers,
            )

            if response.status_code == 200:
                data: dict[str, Any] = response.json()
                full_text = str(data.get("text", "")).strip()

                if not full_text:
                    return "No content found."

                soup = BeautifulSoup(full_text, "html.parser")
                full_text = soup.get_text()

                # Ignore content after the history separator
                history_separator = "============================================"
                if history_separator in full_text:
                    return full_text.split(history_separator)[0].strip()

                return full_text

            return f"Error: Received status code {response.status_code}"

        except httpx.RequestError as e:
            log.error(f"Network error fetching message body: {e}")
            log.debug("Network error details", exc_info=True)
            return "Error retrieving content."
        except Exception as e:
            log.error(f"Unexpected error: {e}")
            log.debug("Unexpected error details", exc_info=True)
            return "Error retrieving content."

    async def _parse_messages(self, url: str) -> list[dict[str, Any]]:
        """Parses the inbox for ASC messages."""
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="Reputação")
        soup = BeautifulSoup(response, "html.parser")

        if not soup:
            return new_items

        inbox_table = soup.find("table", class_="table-inbox")
        if not inbox_table:
            return new_items

        message_rows = inbox_table.find_all("tr")

        for row in message_rows:
            item_id = row.get("id")
            if not item_id:
                continue

            item_id = str(item_id)

            if item_id in self.state["processed_ids"]:
                continue

            sender_cell = row.find("td", class_="inbox-small-cells")
            sender = "System"
            if sender_cell:
                nick_tag = sender_cell.find("span", class_="nick")
                if nick_tag:
                    sender = nick_tag.get_text(strip=True)

            subject_cell = row.find("td", class_="view-message")
            subject = "No Subject"
            if subject_cell:
                subject_link = subject_cell.find("a")
                if subject_link:
                    subject = subject_link.get_text(strip=True)

            date_cell = row.find_all("td", class_="view-message")
            date_str = "Unknown"
            if len(date_cell) >= 2:
                date_str = date_cell[1].get_text(strip=True)

            link = urljoin(self.base_url, f"mensagens.php?action=ver_mp&id_mp={item_id}")

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "sender": sender,
                    "subject": subject,
                    "date": date_str,
                    "url": link,
                    "is_staff": False,
                }
            )

        return new_items
