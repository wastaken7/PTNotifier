#!/usr/bin/env python3

from pathlib import Path
from typing import Any, Callable, Optional

from bs4 import BeautifulSoup

import config

from .base import BaseTracker, log


class UNIT3D(BaseTracker):
    """
    Manages a session for UNIT3D based trackers.
    """
    def __init__(self, cookie_path: Path):
        self.cookie_path = cookie_path
        self.domain = self._extract_domain_from_cookie(cookie_path)
        super().__init__(
            cookie_path,
            tracker_name=self.domain,
            base_url=f"https://{self.domain}",
        )

        self.headers.update(
            {
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,image/apng,*/*;q=0.8",
                "Accept-Language": "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
                "Connection": "keep-alive",
            }
        )
        if self.domain == "eiga.moi" or self.domain == "hawke.uno":
            self.notifications_url = f"https://{self.domain}/notifications"
            self.messages_url = f"https://{self.domain}/mail/inbox"
        else:
            self.notifications_url = self.state.get("notifications_url", "")
            self.messages_url = self.state.get("messages_url", "")
        self.csrf_token: Optional[str] = self.state.get("csrf_token")

    async def initialize(self):
        if not self.domain:
            log.error(f"Initialization failed: Could not determine domain from cookies.: {self.cookie_path}")
            return

        if self.notifications_url:
            if self.messages_url:
                return
            else:
                self.state["messages_url"] = self.messages_url.replace("notifications", "conversations")
                self._save_state()
                return

        response = await self._fetch_page(self.base_url, "user ID")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            log.error(f"{self.domain}: Initialization failed.")
            return

        token_tag = soup.find("meta", {"name": "csrf-token"})
        if token_tag:
            content = token_tag.get("content")
            if isinstance(content, list):
                self.csrf_token = content[0] if content else None
            else:
                self.csrf_token = content

        notif_link = soup.find("a", href=lambda href: bool(href and "notifications" in href))
        if notif_link:
            href = notif_link.get("href")
            self.notifications_url = self._make_absolute_url(href[0] if isinstance(href, list) else str(href))

        msg_link = soup.find("a", href=lambda href: bool(href and "conversations" in href))
        if msg_link:
            href = msg_link.get("href")
            self.messages_url = self._make_absolute_url(href[0] if isinstance(href, list) else str(href))

        if self.notifications_url or self.messages_url or self.csrf_token:
            self.state["notifications_url"] = self.notifications_url
            self.state["messages_url"] = self.messages_url
            self.state["csrf_token"] = self.csrf_token
            self._save_state()

    def _make_absolute_url(self, href: str) -> str:
        if href.startswith("http"):
            return href
        return f"{self.base_url.rstrip('/')}/{href.lstrip('/')}"

    async def _fetch_and_parse(
        self,
        url: str,
        parse_func: Callable[[BeautifulSoup], list[dict[str, Any]]],
        request_type: str,
    ) -> list[dict[str, Any]]:
        if not url:
            return []
        response = await self._fetch_page(url, request_type, sucess_text="general-settings")
        soup = BeautifulSoup(response, "html.parser")
        if soup:
            return parse_func(soup)
        return []

    def _parse_notifications_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        unread_cells = soup.find_all("td", class_="notification--unread")
        if not unread_cells:
            log.error(f"{self.domain}: Error parsing HTML. This likely means that this is a heavily modified version of UNIT3D or that your cookies have expired.")
            return []

        items: list[dict[str, Any]] = []

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
                    "subject": cols[1].get_text(" ", strip=True),
                    "date": cols[2].get_text(" ", strip=True),
                    "url": action_url_str,
                }
            )
        return items

    def _parse_messages_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
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

            items.append(
                {
                    "type": "message",
                    "id": msg_id,
                    "sender": sender,
                    "subject": subject,
                    "date": cols[2].get_text(strip=True),
                    "url": str(msg_url),
                }
            )
        return items

    async def _fetch_body(self, url: str) -> str:
        """
        Fetches the content of the last message/comment from a specific page.
        """
        try:
            response = await self._fetch_page(url, "item body")
            soup = BeautifulSoup(response, "html.parser")

            bodies = soup.find_all("div", class_="panel__body bbcode-rendered")
            if bodies:
                return bodies[-1].get_text(separator="\n\n", strip=True)
        except Exception as e:
            log.error(f"{self.domain}: Failed to fetch body from {url}:", exc_info=e)

        return ""

    async def _mark_as_read(self, item: dict[str, Any]) -> bool:
        if item["type"] == "message":
            return True

        if not self.csrf_token or not config.SETTINGS.get("MARK_AS_READ", False):
            return True

        payload = {"_token": self.csrf_token, "_method": "PATCH"}
        headers: dict[str, str] = {
            **self.headers,
            "Content-Type": "application/x-www-form-urlencoded",
            "Referer": self.notifications_url,
        }
        try:
            resp = await self.client.post(item["url"], data=payload, headers=headers)
            return resp.is_success
        except Exception as e:
            log.error(f"{self.domain}: Exception marking as read:", exc_info=e)
            return False

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch all new items from the tracker and populate their bodies."""
        await self.initialize()

        notifs = await self._fetch_and_parse(self.notifications_url, self._parse_notifications_html, "notifications")
        msgs = await self._fetch_and_parse(self.messages_url, self._parse_messages_html, "messages")

        all_items = notifs + msgs

        for item in all_items:
            if item["type"] == "message":
                item["body"] = await self._fetch_body(item["url"])

        return all_items

    async def _ack_item(self, item: dict[str, Any]) -> None:
        """Marks an item as processed and read on the site."""
        await self._mark_as_read(item)
        await super()._ack_item(item)
