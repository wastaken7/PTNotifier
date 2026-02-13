#!/usr/bin/env python3

import json
import re
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

import config

from .base import BaseTracker, log


class Orpheus(BaseTracker):
    """
    Manages a session for Orpheus.network.
    Uses Gazelle-style AJAX API.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="Orpheus",
            base_url="https://orpheus.network/",
        )
        self.headers.update({"Accept": "application/json"})
        self.api_token = config.API_TOKENS.get("Orpheus")
        if self.api_token:
            self.headers.update({"Authorization": f"token {self.api_token}"})

        self.inbox_api = urljoin(self.base_url, "ajax.php?action=inbox")
        self.conv_api = urljoin(self.base_url, "ajax.php?action=inbox&type=viewconv&id=")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages from Orpheus API."""
        if not self.api_token:
            log.warning(f"{self.tracker}: API Token not found in config. Skipping...")
            return []

        return await self._fetch_mailbox()

    async def _fetch_mailbox(self) -> list[dict[str, Any]]:
        """Parses the inbox AJAX response."""
        new_items: list[dict[str, Any]] = []
        raw_data = await self._fetch_page(self.inbox_api, "messages", success_text='"status":"success"')
        if not raw_data:
            return new_items

        try:
            data = json.loads(raw_data)
        except Exception:
            log.error(f"{self.tracker}: Failed to parse inbox JSON.")
            log.debug(f"{self.tracker}: Raw data: {raw_data}", exc_info=True)
            return new_items

        if data.get("status") != "success":
            return new_items

        messages = data.get("response", {}).get("messages", [])

        for msg in messages:
            try:
                conv_id = str(msg.get("convId"))
                if not conv_id or conv_id in self.state["processed_ids"]:
                    continue

                body = await self._fetch_conversation_body(conv_id)

                sender = msg.get("username", "System")
                subject = msg.get("subject", "No Subject")
                link = urljoin(self.base_url, f"inbox.php?action=viewconv&id={conv_id}")

                clean_body = re.sub(r"<[^>]*>", "", body).strip()

                new_items.append(
                    {
                        "type": "message",
                        "id": conv_id,
                        "sender": sender,
                        "subject": subject,
                        "body": clean_body,
                        "date": msg.get("date", ""),
                        "url": link,
                    }
                )
            except Exception:
                log.error(f"{self.tracker}: Failed to process message: {msg}", exc_info=True)
                continue
        return new_items

    async def _fetch_conversation_body(self, conv_id: str) -> str:
        """Fetches the actual text content of a specific conversation."""
        url = f"{self.conv_api}{conv_id}"
        raw_data = await self._fetch_page(url, f"conversation {conv_id}")

        try:
            data = json.loads(raw_data)
            if data.get("status") == "success":
                messages = data.get("response", {}).get("messages", [])
                if messages:
                    return messages[-1].get("body", "")
        except Exception:
            pass

        return ""
