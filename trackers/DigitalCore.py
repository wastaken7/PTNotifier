#!/usr/bin/env python3

import re
import time
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from rich.console import Console

import config

from .base import BaseTracker

console = Console()


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

    async def _fetch_api(self, url: str) -> Any:
        """Fetches data from an API endpoint."""
        config_timeout = config.SETTINGS.get("TIMEOUT", 30.0)
        try:
            config_timeout = float(str(config_timeout))
        except (ValueError, TypeError):
            config_timeout = 30
        try:
            console.print(f"{self.tracker}: [blue]Checking for {'messages' if 'mailbox' in url else 'notifications'}...[/blue]")
            response = await self.client.get(url, timeout=config_timeout)
            response.raise_for_status()
            return response.json()
        except Exception as e:
            console.print(f"{self.tracker}: [bold red]Error fetching API {url}:[/bold red] {e}")
            return None

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch messages and notifications from DigitalCore API."""
        mailbox_items = await self._fetch_mailbox()
        notification_items = await self._fetch_notifications()
        return mailbox_items + notification_items

    async def _fetch_mailbox(self) -> list[dict[str, Any]]:
        """Parses the mailbox API response."""
        new_items: list[dict[str, Any]] = []
        data = await self._fetch_api(self.mailbox_api)
        if not data:
            return new_items

        for msg in data:
            item_id = str(msg.get("id"))
            if item_id in self.state["processed_ids"]:
                continue

            sender = msg.get("user", {}).get("username", "System")
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
        data = await self._fetch_api(self.notifications_api)
        if not data:
            return new_items

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
