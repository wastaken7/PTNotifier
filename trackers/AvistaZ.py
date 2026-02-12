#!/usr/bin/env python3

from pathlib import Path
from typing import Any

from bs4 import BeautifulSoup

from .base import BaseTracker, log


class AvistaZ(BaseTracker):
    """
    Manages a session for the AvistaZ network.
    """

    def __init__(self, cookie_path: Path):
        self.domain = self._extract_domain_from_cookie(cookie_path)
        super().__init__(
            cookie_path,
            self.domain,
            f"https://{self.domain}/",
        )

        self.notifications_url = self.base_url + "notifications"
        self.messages_url = self.base_url + "messenger"

    def get_favicon(self) -> str:
        if "privatehd" in self.domain:
            return "https://privatehd.to/images/privatehd-favicon.png"
        elif "cinemaz" in self.domain:
            return "https://cinemaz.to/images/cinemaz-favicon.png"
        elif "avistaz" in self.domain:
            return "https://avistaz.to/images/avistaz-favicon.png"
        elif "exoticaz" in self.domain:
            return "https://i.exoticaz.to/images/favicon.png"
        else:
            return "https://avistaz.to/images/avistaz-favicon.png"

    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch all new items from the tracker."""
        notifications = await self._fetch_and_parse_notifications()
        messages = await self._fetch_and_parse_messages()
        return notifications + messages

    async def _fetch_and_parse_notifications(self) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(self.notifications_url, "notifications")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return new_items

        table = soup.find("table", class_="table-hover")
        if not table:
            return new_items

        tbody = table.find("tbody")
        if not tbody:
            return new_items

        rows = tbody.find_all("tr")
        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 3:
                continue

            date_posted = cols[0].get_text(strip=True)
            title_cell = cols[1].find("a")
            title = title_cell.get_text(strip=True) if title_cell else cols[1].get_text(strip=True)
            link = title_cell["href"] if title_cell else self.notifications_url
            notification_msg = cols[2].get_text(strip=True)

            if link in self.state["processed_ids"]:
                continue

            new_items.append(
                {
                    "favicon": self.get_favicon(),
                    "type": "notification",
                    "id": link,
                    "title": title,
                    "subject": notification_msg,
                    "date": date_posted,
                    "url": link,
                }
            )
        return new_items

    async def _fetch_and_parse_messages(self) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(self.messages_url, "messages", sucess_text="messenger/new")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return new_items

        table = soup.find("table", class_="table-hover")
        if not table:
            return new_items

        tbody = table.find("tbody")
        if not tbody:
            return new_items

        rows = tbody.find_all("tr", class_="info text-bold")
        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 5:
                continue

            sender = cols[2].get_text(strip=True)
            subject_cell = cols[3].find("a")
            if not subject_cell:
                continue

            subject = subject_cell.get_text(strip=True)
            link = str(subject_cell["href"])
            age = cols[4].get_text(strip=True)

            body = await self._fetch_body(link)

            new_items.append(
                {
                    "favicon": self.get_favicon(),
                    "type": "message",
                    "id": link,
                    "sender": sender,
                    "subject": subject,
                    "body": body,
                    "date": age,
                    "url": link,
                }
            )
        return new_items

    async def _fetch_body(self, url: str) -> str:
        """
        Fetches and parses the content of a specific message.
        """
        try:
            response = await self._fetch_page(url, "message body")
            soup = BeautifulSoup(response, "html.parser")

            body_div = soup.find("div", class_="torrent-desc")
            if body_div:
                return body_div.get_text(separator="\n\n", strip=True)

            return "No content found."
        except Exception as e:
            log.error(f"Error fetching message body from {url}", exc_info=e)
            return "Error retrieving message body."
