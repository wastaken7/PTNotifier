#!/usr/bin/env python3

import asyncio
import glob
import re
import time
from collections.abc import Coroutine
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urljoin

from rich.console import Console

from .base import BaseTracker

console = Console()


class DC(BaseTracker):
    """
    Manages a session for DigitalCore using API endpoints and cookie files.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path, "DC", "https://digitalcore.club/")
        self.headers.update({"Accept": "application/json, text/plain, */*"})

        self.mailbox_api = urljoin(self.base_url, "/api/v1/mailbox?index=0&limit=20&location=0")
        self.notifications_api = urljoin(self.base_url, "/api/v1/notifications?index=0&limit=20&toReturn=all")

    async def _fetch_api(self, url: str) -> Any:
        """Fetches data from an API endpoint."""
        try:
            response = await self.client.get(url)
            response.raise_for_status()
            return response.json()
        except Exception as e:
            console.print(f"{self.tracker}: Error fetching API {url}: {e}")
            return None

    async def process(
        self,
        send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]],
    ):
        """Main loop to fetch messages and notifications from DigitalCore API."""
        try:
            mailbox_items = await self._fetch_mailbox()
            notification_items = await self._fetch_notifications()

            all_items = mailbox_items + notification_items

            for item in all_items:
                if not self.first_run:
                    await send_telegram(item, self.base_url, item["url"])
                    await asyncio.sleep(3)
                self._ack_item(item["id"])

        except Exception as e:
            console.print(f"{self.tracker}: Error processing {self.base_url}: {e}")
        finally:
            await self.client.aclose()

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

            new_items.append(
                {
                    "type": "message",
                    "id": item_id,
                    "title": sender,
                    "msg": subject,
                    "body": msg.get("body", ""),
                    "date": msg.get("added", ""),
                    "url": link,
                }
            )
        return new_items

    async def _fetch_notifications(self) -> list[dict[str, Any]]:
        """Parses the notifications API response (tips, thanks, etc)."""
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
                    "title": "Notification",
                    "msg": clean_msg,
                    "date": date_str,
                    "url": urljoin(self.base_url, "/notifications"),
                }
            )
        return new_items

    @staticmethod
    async def fetch_notifications(send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]]):
        """Module-level entry point for DigitalCore."""
        cookie_files = glob.glob(str(Path("./cookies") / "OTHER" / "DC.txt"))
        if not cookie_files:
            return

        sessions = [DC(Path(f)) for f in cookie_files]
        tasks = [session.process(send_telegram) for session in sessions]
        await asyncio.gather(*tasks)
