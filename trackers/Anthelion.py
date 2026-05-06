#!/usr/bin/env python3

import json
import re
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urljoin

import config
from utils.console import log

from .base import BaseTracker


class Anthelion(BaseTracker):
    """
    Manages a session for Anthelion.
    Uses the official Anthelion API for inbox messages.
    """

    api_only = True

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="Anthelion",
            base_url="https://anthelion.me/",
        )
        self.api_key = config.API_TOKENS.get("Anthelion")
        self.inbox_api = f"{self.base_url}api.php?action=inbox&api_key={self.api_key}"

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """
        Fetch messages from the Anthelion API.

        Returns:
            list[dict[str, Any]]: List of messages with their bodies.
        """
        if not self.api_key:
            log.warning(f"{self.tracker}: API key not found in config. Skipping...")
            return []

        return await self._fetch_inbox()

    async def _fetch_test_item(self) -> Optional[dict[str, Any]]:
        """
        Fetch a single test item (the first unread message or first read message if no unread).

        Returns:
            dict[str, Any] | None: The test item with its body, or None if no items are found.
        """
        if not self.api_key:
            log.warning(f"{self.tracker}: API key not found in config. Skipping...")
            return None

        unread_items = await self._fetch_inbox()
        if unread_items:
            return unread_items[0]

        fallback_items = await self._fetch_inbox(ignore_processed=True)
        if fallback_items:
            return fallback_items[0]

        return None

    async def _fetch_inbox(self, ignore_processed: bool = False) -> list[dict[str, Any]]:
        """
        Fetches the inbox API response and returns new message items.

        Args:
            ignore_processed (bool): Whether to ignore processed messages.

        Returns:
            list[dict[str, Any]]: List of messages.
        """
        new_items: list[dict[str, Any]] = []
        raw_data = await self._fetch_page(self.inbox_api, "messages", success_text='"status": "success"')
        if not raw_data:
            return new_items

        try:
            data = json.loads(raw_data)
        except Exception:
            log.error(f"{self.tracker}: Failed to parse inbox JSON.")
            log.debug(f"{self.tracker}: Raw data: {raw_data}", exc_info=True)
            return new_items

        if data.get("status") != "success":
            log.error(f"{self.tracker}: API returned non-success status.")
            return new_items

        messages = data.get("response", {}).get("messages", [])

        for msg in messages:
            try:
                message_id = str(msg.get("message_id", ""))
                if not message_id:
                    continue

                if not ignore_processed and message_id in self.state["processed_ids"]:
                    continue

                conv_id = str(msg.get("conv_id", ""))
                sender = msg.get("sender", "System")
                subject = msg.get("subject", "No Subject")
                body = msg.get("body", "")
                if isinstance(body, bool):
                    body = ""
                else:
                    body = re.sub(r"\[\[([^\]]+)\]\]", r"\1", body)
                    body = re.sub(r"\[torrent\]([^\[]+)\[/torrent\]", r"https://anthelion.me/torrents.php?id=\1", body)
                    body = re.sub(r"\[/?[^\]]+\]", "", body).strip()
                date = msg.get("sent_date", "")
                link = urljoin(self.base_url, f"inbox.php?action=viewconv&id={conv_id}")

                new_items.append(
                    {
                        "type": "message",
                        "id": message_id,
                        "sender": sender,
                        "subject": subject,
                        "body": body,
                        "date": date,
                        "url": link,
                    }
                )
            except Exception as e:
                log.error(f"{self.tracker}: Failed to process message: {e}")
                log.debug(f"{self.tracker}: Raw data: {msg}", exc_info=True)
                continue

        return new_items
