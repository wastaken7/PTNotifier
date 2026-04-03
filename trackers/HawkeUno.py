#!/usr/bin/env python3
from pathlib import Path
from typing import Any

from bs4 import BeautifulSoup

from .UNIT3D import UNIT3D


class HawkeUno(UNIT3D):
    """
    Custom tracker for Hawke-Uno (Deep Space) with auto-discovery.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path)

    def _discover_username(self, soup: BeautifulSoup):
        if self.state.get("username"):
            return

        user_tag = soup.find("a", class_="deep-space-user-card__header-username")
        if user_tag:
            username = user_tag.get_text(strip=True)
            self.state["username"] = username

    def _parse_notifications_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        self._discover_username(soup)

        unread_rows = soup.find_all("tr", class_="ds-macro-row--unread")
        items: list[dict[str, Any]] = []

        for row in unread_rows:
            name_td = row.find("td", class_="ds-macro-row__name")
            if not name_td:
                continue

            link_tag = name_td.find("a")
            if not link_tag:
                continue

            title = link_tag.get_text(strip=True)
            url = link_tag.get("href", "")

            cols = row.find_all("td")
            date_text = cols[1].get_text(strip=True) if len(cols) > 1 else ""

            wire_key = row.get("wire:key", "")
            notif_id = wire_key if wire_key else f"notif_{hash(title)}"

            if notif_id in self.state["processed_ids"]:
                continue

            items.append(
                {
                    "type": "notification",
                    "id": notif_id,
                    "title": title,
                    "date": date_text,
                    "url": f"https://hawke.uno{url}",
                }
            )
        return items

    def _parse_messages_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        self._discover_username(soup)
        username = self.state.get("username", "me")

        items: list[dict[str, Any]] = []
        rows = soup.find_all("div", class_="deep-space-messages__row")

        for row in rows:
            username_tag = row.find("span", class_="deep-space-messages__row-username")
            if not username_tag:
                continue

            sender = username_tag.get_text(strip=True)
            preview_tag = row.find("div", class_="deep-space-messages__row-preview")
            subject = preview_tag.get_text(strip=True) if preview_tag else "New Message"

            wire_key = row.get("wire:key", "")
            correspondent_id = wire_key.split("-")[-1] if "-" in wire_key else ""

            msg_url = f"https://hawke.uno/users/{username}/hub/messages"
            if correspondent_id:
                msg_url += f"?activeCorrespondentId={correspondent_id}"

            items.append(
                {
                    "type": "message",
                    "id": f"msg_{correspondent_id}",
                    "sender": sender,
                    "subject": subject,
                    "date": row.find("span", class_="deep-space-messages__row-time").get_text(strip=True),
                    "url": msg_url,
                }
            )
        return items
