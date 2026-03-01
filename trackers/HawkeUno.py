#!/usr/bin/env python3
from pathlib import Path
from typing import Any

from bs4 import BeautifulSoup

from .UNIT3D import UNIT3D


class HawkeUno(UNIT3D):
    """
    Custom tracker for Hawke-Uno.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(cookie_path)

    def _parse_notifications_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        unread_rows = soup.find_all("tr", class_="success")

        items: list[dict[str, Any]] = []

        for row in unread_rows:
            cols = row.find_all("td")
            if len(cols) < 4:
                continue

            link_tag = cols[0].find("a")
            if not link_tag:
                continue

            title_tag = link_tag.find("span", class_="notification-title")
            if not title_tag:
                continue

            form = cols[2].find("form")
            action_url = form.get("action") if form else None
            if not action_url:
                continue

            date_tag = cols[1].find("span", class_="notification-ago")

            action_url_str = str(action_url)
            notif_id = f"notif_{action_url_str.rstrip('/').split('/')[-2]}"
            if notif_id in self.state["processed_ids"]:
                continue

            items.append(
                {
                    "type": "notification",
                    "id": notif_id,
                    "title": title_tag.get_text(" ", strip=True),
                    "date": date_tag.get_text(" ", strip=True) if date_tag else "",
                    "url": action_url_str,
                }
            )
            pass
        return items

    def _parse_messages_html(self, soup: BeautifulSoup) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        rows = soup.find_all("tr")

        for row in rows:
            if "color: grey;" in str(row):
                continue

            cols = row.find_all("td")
            if len(cols) < 5:
                continue

            sender_tag = cols[1].find("a")
            if not sender_tag:
                continue
            sender = sender_tag.get_text(strip=True)

            subject_tag = cols[2].find("a")
            if not subject_tag:
                continue
            subject = subject_tag.get_text(strip=True)

            msg_id = f"msg_{str(subject_tag['href']).rstrip('/').split('/')[-1]}"
            msg_url = subject_tag["href"]

            items.append(
                {
                    "type": "message",
                    "id": msg_id,
                    "sender": sender,
                    "subject": subject,
                    "date": cols[3].get_text(strip=True),
                    "url": str(msg_url),
                }
            )
        return items
