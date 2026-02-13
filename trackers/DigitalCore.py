#!/usr/bin/env python3

import json
import re
import time
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from .base import BaseTracker, log


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
        self.notifications_api = urljoin(self.base_url, "/api/v1/notifications?index=0&limit=20&toReturn=all")

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages and notifications from DigitalCore API."""
        mailbox_items = await self._fetch_mailbox()
        notification_items = await self._fetch_notifications()
        return mailbox_items + notification_items

    async def _fetch_mailbox(self) -> list[dict[str, Any]]:
        """Parses the mailbox API response."""
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

    async def _fetch_notifications(self) -> list[dict[str, Any]]:
        """Parses the notifications API response."""
        new_items: list[dict[str, Any]] = []
        data = await self._fetch_page(self.mailbox_api, "notifications")

        try:
            data = json.loads(data)
        except Exception as e:
            log.error(f"{self.tracker}: Failed to parse notifications JSON: {e}")
            log.debug(f"{self.tracker}: Raw data: {data}", exc_info=True)

            return new_items

        if not data:
            return new_items

        notif: dict[str, Any]
        for notif in data:
            item_id = f"notif_{notif.get('id')}"
            if item_id in self.state["processed_ids"]:
                continue

            sent_time = notif.get("sentTime", 0)
            date_str = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(sent_time)) if sent_time else "Unknown"

            raw_msg = notif.get("message", "")
            clean_msg = re.sub(r"\[.*?\]", "", raw_msg).strip()

            new_items.append(
                {
                    "type": "notification",
                    "id": item_id,
                    "subject": clean_msg,
                    "date": date_str,
                    "url": urljoin(self.base_url, "/notifications"),
                }
            )
        return new_items
