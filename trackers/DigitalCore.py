#!/usr/bin/env python3

import json
import re
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from utils.console import log

from .base import BaseTracker


class DigitalCore(BaseTracker):
    """
    Manages a session for DigitalCore.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="DigitalCore",
            base_url="https://digitalcore.club/",
        )
        self.headers.update({"Accept": "application/json, text/plain, */*"})

        self.mailbox_api = urljoin(self.base_url, "/api/v1/mailbox?index=0&limit=20&location=0")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """
        Fetch messages from DigitalCore API.

        Returns:
            list[dict[str, Any]]: List of items.
        """
        return await self._fetch_mailbox()

    async def _fetch_mailbox(self) -> list[dict[str, Any]]:
        """
        Parses the mailbox API response.

        Returns:
            list[dict[str, Any]]: List of items.
        """
        new_items: list[dict[str, Any]] = []
        data = await self._fetch_page(self.mailbox_api, "messages")
        if not data:
            return new_items

        try:
            data = json.loads(data)
        except Exception:
            log.error(f"{self.tracker}: Failed to parse inbox JSON.")
            log.debug(f"{self.tracker}: Raw data: {data}", exc_info=True)
            return new_items

        if not data:
            return new_items

        msg: dict[str, Any]
        for msg in data:
            item_id = str(msg.get("id"))
            if item_id in self.state["processed_ids"]:
                continue

            sender = dict(msg.get("user", {})).get("username", "System")
            subject = msg.get("subject", "No Subject")
            link = urljoin(self.base_url, f"/mailbox/{item_id}")
            clean_body = re.sub(r"\[.*?\]", "", msg.get("body", "")).strip()

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "sender": sender,
                    "subject": subject,
                    "body": clean_body,
                    "date": msg.get("added", ""),
                    "url": link,
                }
            )
        return new_items
