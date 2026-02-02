#!/usr/bin/env python3

from pathlib import Path
from typing import Any, Callable, Optional

from bs4 import BeautifulSoup
from rich.console import Console

import config

from .base import BaseTracker

console = Console()


class UNIT3D(BaseTracker):
    def __init__(self, cookie_path: Path):
        self.domain = self._extract_domain_from_cookie(cookie_path)
        super().__init__(cookie_path, self.domain, f"https://{self.domain}")

        self.headers.update(
            {
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,image/apng,*/*;q=0.8",
                "Accept-Language": "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
                "Connection": "keep-alive",
            }
        )
        self.notifications_url = self.state.get("notifications_url", "")
        self.messages_url = self.state.get("messages_url", "")
        self.csrf_token: Optional[str] = self.state.get("csrf_token")

    async def initialize(self):
        if not self.domain:
            return

        if self.notifications_url and self.messages_url:
            return

        soup = await self._fetch_page(self.base_url)
        if not soup:
            console.print(f"{self.domain}: [bold red]Initialization failed.[/bold red]")
            return

        token_tag = soup.find("meta", {"name": "csrf-token"})
        if token_tag:
            self.csrf_token = token_tag.get("content")

        notif_link = soup.find("a", href=lambda href: href and "notifications" in href)
        if notif_link:
            self.notifications_url = self._make_absolute_url(notif_link["href"])

        msg_link = soup.find("a", href=lambda href: href and "conversations" in href)
        if msg_link:
            self.messages_url = self._make_absolute_url(msg_link["href"])

        if self.notifications_url or self.messages_url or self.csrf_token:
            self.state["notifications_url"] = self.notifications_url
            self.state["messages_url"] = self.messages_url
            self.state["csrf_token"] = self.csrf_token
            self._save_state()

    def _make_absolute_url(self, href: str) -> str:
        if href.startswith("http"):
            return href
        return f"{self.base_url.rstrip('/')}/{href.lstrip('/')}"

    async def _fetch_and_parse(self, url: str, parse_func: Callable):
        if not url:
            return []
        soup = await self._fetch_page(url)
        if soup:
            return parse_func(soup)
        return []

    def _parse_notifications_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        unread_cells = soup.find_all("td", class_="notification--unread")
        items = []

        for cell in unread_cells:
            row = cell.find_parent("tr")
            if not row:
                continue
            cols = row.find_all("td")
            if len(cols) < 4:
                continue

            form = cols[3].find("form")
            action_url = form.get("action") if form else None
            if not action_url:
                continue

            action_url_str = str(action_url)
            notif_id = f"notif_{action_url_str.rstrip('/').split('/')[-1]}"
            if notif_id in self.state["processed_ids"]:
                continue

            items.append(
                {
                    "type": "notification",
                    "id": notif_id,
                    "title": cols[0].get_text(" ", strip=True),
                    "msg": cols[1].get_text(" ", strip=True),
                    "date": cols[2].get_text(" ", strip=True),
                    "url": action_url_str,
                }
            )
        return items

    def _parse_messages_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        items = []
        rows = soup.find_all("tr")

        for row in rows:
            unread_icon = row.find("i", class_="text-red")
            if not unread_icon:
                continue

            cols = row.find_all("td")
            if len(cols) < 6:
                continue

            sender = cols[0].get_text(strip=True)
            link_tag = cols[1].find("a")
            if not link_tag:
                continue

            subject = link_tag.get_text(strip=True)
            msg_url = link_tag["href"]
            if isinstance(msg_url, list):
                msg_url = msg_url[0]
            msg_id = f"msg_{str(msg_url).rstrip('/').split('/')[-1]}"

            if msg_id in self.state["processed_ids"]:
                continue

            items.append(
                {
                    "type": "message",
                    "id": msg_id,
                    "title": sender,
                    "msg": subject,
                    "date": cols[2].get_text(strip=True),
                    "url": str(msg_url),
                }
            )
        return items

    async def _mark_as_read(self, item: dict) -> bool:
        if item["type"] == "message":
            return True

        if not self.csrf_token or not config.SETTINGS.get("MARK_AS_READ", False):
            return True

        payload = {"_token": self.csrf_token, "_method": "PATCH"}
        headers = {
            **self.headers,
            "Content-Type": "application/x-www-form-urlencoded",
            "Referer": self.notifications_url,
        }
        try:
            resp = await self.client.post(item["url"], data=payload, headers=headers)
            return resp.is_success
        except Exception as e:
            console.print(f"{self.domain}: [bold red]Exception marking as read:[/bold red] {e}")
            return False

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch all new items from the tracker."""
        await self.initialize()
        notifs = await self._fetch_and_parse(self.notifications_url, self._parse_notifications_html)
        msgs = await self._fetch_and_parse(self.messages_url, self._parse_messages_html)
        return notifs + msgs

    async def _ack_item(self, item: dict[str, Any]) -> None:
        """Marks an item as processed and read on the site."""
        await self._mark_as_read(item)
        await super()._ack_item(item)
