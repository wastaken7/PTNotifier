#!/usr/bin/env python3

import copy
import re
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urljoin

from bs4 import BeautifulSoup, Tag
import httpx
import pyotp

import config

from utils.console import log

from .base import BaseTracker


class MTV(BaseTracker):
    """
    Manages a session for MoreThanTV.
    """

    def __init__(self, cookie_path: Path):
        super().__init__(
            cookie_path,
            tracker_name="MTV",
            base_url="https://www.morethantv.me/",
        )

        self.login_url = urljoin(self.base_url, "login")
        self.twofactor_url = urljoin(self.base_url, "twofactor/login")
        self.inbox_url = urljoin(self.base_url, "user/inbox/received?sort=unread")
        self.staff_url = urljoin(self.base_url, "staffpm.php?action=user_inbox")
        self.notifications_url: str = self.state.get("notifications_url", "")
        self._relogin_attempted = False

    @property
    def tracker_settings(self) -> dict[str, Any]:
        tracker_settings = getattr(config, "TRACKER_SETTINGS", {})
        if isinstance(tracker_settings, dict):
            mtv_settings = tracker_settings.get("MTV", {})
            if isinstance(mtv_settings, dict):
                return mtv_settings
        return {}

    async def _fetch_page(
        self,
        url: str,
        request_type: str,
        success_text: str = "",
    ) -> str:
        response = await super()._fetch_page(url, request_type, success_text)
        if not self._needs_relogin(response, success_text):
            self._relogin_attempted = False
            return response

        if self._relogin_attempted:
            log.error(f"{self.tracker}: Login refresh failed while fetching {request_type}.")
            return response

        if not self._has_login_credentials():
            log.warning(
                f"{self.tracker}: Session appears expired and no MTV login credentials are configured. "
                "Add TRACKER_SETTINGS['MTV'] with username/password and optional otp_uri for automatic relogin."
            )
            return response

        self._relogin_attempted = True
        if not await self._login():
            log.error(f"{self.tracker}: Automatic relogin failed.")
            return response

        retry_response = await super()._fetch_page(url, request_type, success_text)
        self._relogin_attempted = False
        return retry_response

    async def _fetch_items(self) -> list[dict[str, Any]]:
        await self._discover_notifications_url()

        notifications = await self._parse_notifications()
        inbox_items = await self._parse_inbox(self.inbox_url)
        staff_items = await self._parse_staff_inbox(self.staff_url)
        return notifications + inbox_items + staff_items

    async def _discover_notifications_url(self) -> None:
        if self.notifications_url:
            return

        try:
            response = await self._fetch_page(self.base_url, "MTV home", success_text="Logout")
        except Exception as e:
            log.error(f"{self.tracker}: Failed to discover notification URL: {e}")
            return

        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return

        for link in soup.find_all("a", href=True):
            href = str(link.get("href", ""))
            href_lower = href.lower()
            if "notification" not in href_lower:
                continue
            if any(excluded in href_lower for excluded in ("ajax", "manage", "mark", "settings")):
                continue

            self.notifications_url = urljoin(self.base_url, href)
            self.state["notifications_url"] = self.notifications_url
            self._save_state()
            return

    async def _parse_notifications(self) -> list[dict[str, Any]]:
        if not self.notifications_url:
            return []

        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(self.notifications_url, "notifications", success_text="Logout")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return new_items

        containers: list[Tag] = []

        table_rows = [
            row for row in soup.find_all("tr")
            if self._is_unread_tag(row) or row.find("strong")
        ]
        containers.extend(table_rows)

        if not containers:
            containers.extend(
                element for element in soup.find_all(["li", "div", "article"]) if self._is_unread_tag(element)
            )

        seen_ids: set[str] = set()
        for container in containers:
            link = self._extract_primary_link(container)
            if not link:
                continue

            absolute_url = urljoin(self.base_url, link)
            item_id = f"notif_{self._extract_numeric_id(absolute_url) or absolute_url}"
            if item_id in self.state["processed_ids"] or item_id in seen_ids:
                continue

            text_parts = list(container.stripped_strings)
            if not text_parts:
                continue

            title = text_parts[0]
            subject = " ".join(text_parts[1:]) if len(text_parts) > 1 else title
            new_items.append(
                {
                    "type": "notification",
                    "id": item_id,
                    "title": title,
                    "subject": subject,
                    "date": self._extract_date_text(container),
                    "url": absolute_url,
                }
            )
            seen_ids.add(item_id)

        return new_items

    async def _parse_inbox(self, url: str) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "messages", success_text="messageform")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return new_items

        rows = self._find_inbox_rows(soup)
        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 4:
                continue

            conv_id = self._extract_checkbox_value(row)
            subject_link = cols[1].find("a", href=True)
            if not subject_link:
                continue

            subject = subject_link.get_text(" ", strip=True)
            thread_url = urljoin(self.base_url, str(subject_link["href"]))
            sender = cols[2].get_text(" ", strip=True)
            date_str = cols[3].get_text(" ", strip=True)

            messages = await self._fetch_conversation_messages(
                thread_url,
                fallback_prefix="pm",
                fallback_id=conv_id,
                subject=subject,
                sender=sender,
                date_str=date_str,
                is_staff=False,
            )
            new_items.extend(messages)

        return new_items

    async def _parse_staff_inbox(self, url: str) -> list[dict[str, Any]]:
        new_items: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "staff messages", success_text="Staff PMs")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return new_items

        rows = self._find_staff_open_rows(soup)
        for row in rows:
            cols = row.find_all("td")
            if len(cols) < 4:
                continue

            subject_link = cols[0].find("a", href=True)
            if not subject_link:
                continue

            thread_url = urljoin(self.base_url, str(subject_link["href"]))
            conv_id = self._extract_numeric_id(thread_url)
            subject = subject_link.get_text(" ", strip=True)
            date_str = cols[1].get_text(" ", strip=True)
            sender = cols[2].get_text(" ", strip=True) or "Staff"

            messages = await self._fetch_conversation_messages(
                thread_url,
                fallback_prefix="staff",
                fallback_id=conv_id,
                subject=subject,
                sender=sender,
                date_str=date_str,
                is_staff=True,
            )
            new_items.extend(messages)

        return new_items

    async def _login(self) -> bool:
        headers = {
            **self.headers,
            "Referer": self.login_url,
            "Origin": self.base_url.rstrip("/"),
        }

        try:
            async with httpx.AsyncClient(headers=headers, timeout=30.0, follow_redirects=True, http2=True) as client:
                login_page = await client.get(self.login_url)
                login_page.raise_for_status()

                token = self._extract_form_token(login_page.text)
                if not token:
                    log.error(f"{self.tracker}: Unable to find login token on MTV login page.")
                    return False

                payload = {
                    "username": str(self.tracker_settings.get("username", "")),
                    "password": str(self.tracker_settings.get("password", "")),
                    "keeploggedin": "1",
                    "cinfo": "1920|1080|24|0",
                    "submit": "login",
                    "iplocked": "1",
                    "token": token,
                }

                login_response = await client.post(self.login_url, data=payload)
                login_response.raise_for_status()

                current_url = str(login_response.url)
                if current_url.endswith("twofactor/login"):
                    otp_code = self._generate_otp_code()
                    if not otp_code:
                        log.error(
                            f"{self.tracker}: MTV requires 2FA and no valid otp_uri is configured in TRACKER_SETTINGS['MTV']."
                        )
                        return False

                    twofactor_token = self._extract_form_token(login_response.text)
                    if not twofactor_token:
                        log.error(f"{self.tracker}: Unable to find two-factor token on MTV login page.")
                        return False

                    login_response = await client.post(
                        self.twofactor_url,
                        data={
                            "token": twofactor_token,
                            "code": otp_code,
                            "submit": "login",
                        },
                    )
                    login_response.raise_for_status()

                if not self._looks_logged_in(login_response.text):
                    log.error(f"{self.tracker}: MTV login did not produce an authenticated session.")
                    return False

                self._persist_httpx_cookies(client)
                return True
        except Exception as e:
            log.error(f"{self.tracker}: Automatic login failed: {e}")
            log.debug(f"{self.tracker}: Login error details", exc_info=True)
            return False

    async def _fetch_conversation_messages(
        self,
        url: str,
        fallback_prefix: str,
        fallback_id: Optional[str],
        subject: str,
        sender: str,
        date_str: str,
        is_staff: bool,
    ) -> list[dict[str, Any]]:
        messages_found: list[dict[str, Any]] = []
        response = await self._fetch_page(url, "message body", success_text="Logout")
        soup = BeautifulSoup(response, "html.parser")
        if not soup:
            return messages_found

        seen_ids: set[str] = set()
        for container in self._find_message_containers(soup, is_staff=is_staff):
            message_id = self._extract_message_id(container, is_staff=is_staff)
            if not message_id:
                continue

            item_id = f"{fallback_prefix}_{message_id}"
            if item_id in self.state["processed_ids"] or item_id in seen_ids:
                continue

            body_text = self._extract_message_body(container)
            if not body_text:
                continue

            message_sender = self._extract_message_sender(container) or sender or ("Staff" if is_staff else "System")
            message_date = self._extract_date_text(container) or date_str

            messages_found.append(
                {
                    "type": "message",
                    "id": item_id,
                    "sender": message_sender,
                    "subject": subject,
                    "body": body_text,
                    "date": message_date,
                    "url": self._build_message_url(url, message_id, is_staff=is_staff),
                    "is_staff": is_staff,
                }
            )
            seen_ids.add(item_id)

        if messages_found or not fallback_id:
            return messages_found

        body_text = self._extract_primary_conversation_body(soup)
        if not body_text:
            return messages_found

        item_id = f"{fallback_prefix}_{fallback_id}"
        if item_id in self.state["processed_ids"]:
            return messages_found

        messages_found.append(
            {
                "type": "message",
                "id": item_id,
                "sender": sender or ("Staff" if is_staff else "System"),
                "subject": subject,
                "body": body_text,
                "date": date_str,
                "url": url,
                "is_staff": is_staff,
            }
        )
        return messages_found

    def _find_message_containers(self, soup: BeautifulSoup, is_staff: bool) -> list[Tag]:
        containers: list[Tag] = []

        if is_staff:
            for head in soup.find_all("div", class_="head"):
                box = head.find_next_sibling("div", class_=lambda value: bool(value and "box" in str(value).lower()))
                if isinstance(box, Tag):
                    containers.append(box)
        else:
            containers.extend(soup.find_all("table", id=lambda value: bool(value and str(value).startswith("post"))))
            if not containers:
                containers.extend(soup.find_all("table", class_=lambda value: bool(value and "forum_post" in str(value).lower())))

        deduped: list[Tag] = []
        seen_keys: set[int] = set()
        for container in containers:
            key = id(container)
            if key in seen_keys:
                continue
            seen_keys.add(key)
            deduped.append(container)
        return deduped

    def _extract_message_id(self, container: Tag, is_staff: bool) -> Optional[str]:
        if is_staff:
            content_div = container.find("div", id=lambda value: bool(value and str(value).startswith("content")))
            if content_div:
                numeric = self._extract_numeric_id(str(content_div.get("id", "")))
                if numeric:
                    return numeric

            header = container.find_previous_sibling("div", class_="head")
            if header:
                for link in header.find_all("a", href=True):
                    href = str(link.get("href", ""))
                    if "#" in href:
                        fragment = href.rsplit("#", 1)[-1]
                        numeric = self._extract_numeric_id(fragment)
                        if numeric:
                            return numeric

            for tag in container.find_all(id=True):
                numeric = self._extract_numeric_id(str(tag.get("id", "")))
                if numeric:
                    return numeric
            return None

        container_id = container.get("id")
        if container_id:
            numeric = self._extract_numeric_id(str(container_id))
            if numeric:
                return numeric

        for link in container.find_all("a", href=True):
            href = str(link.get("href", ""))
            if href.startswith("#"):
                numeric = self._extract_numeric_id(href)
                if numeric:
                    return numeric

        for tag in container.find_all(id=True):
            numeric = self._extract_numeric_id(str(tag.get("id", "")))
            if numeric:
                return numeric
        return None

    def _extract_message_body(self, container: Tag) -> str:
        post_content = container.find("div", class_="post_content")
        if post_content:
            return post_content.get_text("\n\n", strip=True)

        body_cell = container.find("td", class_="postbody")
        if body_cell:
            content = body_cell.find("div", class_="post_content")
            if content:
                return content.get_text("\n\n", strip=True)
            return body_cell.get_text("\n\n", strip=True)

        body_cell = container.find("td", class_="body")
        if body_cell:
            preview = body_cell.find(id="contentpreview")
            if preview:
                preview.decompose()
            return body_cell.get_text("\n\n", strip=True)

        for body_div in container.find_all("div", class_=lambda value: bool(value and "body" in str(value).lower())):
            text = body_div.get_text("\n\n", strip=True)
            if text:
                return text

        return container.get_text("\n\n", strip=True)

    def _extract_primary_conversation_body(self, soup: BeautifulSoup) -> str:
        table = soup.find("table", id=lambda value: bool(value and str(value).startswith("post")))
        if table:
            return self._extract_message_body(table)

        table = soup.find("table", class_=lambda value: bool(value and "forum_post" in str(value).lower()))
        if table:
            return self._extract_message_body(table)

        body_div = soup.find("div", class_="body")
        if body_div:
            return body_div.get_text("\n\n", strip=True)
        return ""

    def _extract_message_sender(self, container: Tag) -> str:
        user_name = container.find_previous_sibling("div", class_="head")
        if user_name:
            sender_name = user_name.find("span", class_="user_name")
            if sender_name:
                sender_text = sender_name.get_text(" ", strip=True)
                if sender_text:
                    return sender_text

        float_left = container.find("span", class_=lambda value: bool(value and "float_left" in str(value).lower()))
        if float_left:
            sender_text = self._extract_sender_from_header(float_left)
            if sender_text:
                return sender_text

        header = container.find("tr", class_=lambda value: bool(value and "smallhead" in str(value).lower()))
        if header:
            sender_link = header.find("a", href=lambda href: bool(href and "/user" in href))
            if sender_link:
                return sender_link.get_text(" ", strip=True)

            text = header.get_text(" ", strip=True)
            if text:
                return text

        previous_head = container.find_previous_sibling("div", class_="head")
        if previous_head:
            sender_link = previous_head.find("a", href=lambda href: bool(href and "user" in href))
            if sender_link:
                return sender_link.get_text(" ", strip=True)
            return previous_head.get_text(" ", strip=True)

        return ""

    def _extract_date_text(self, container: Tag) -> str:
        for time_tag in container.find_all(["time", "span", "td", "div"]):
            classes = " ".join(time_tag.get("class", [])) if isinstance(time_tag, Tag) else ""
            classes_lower = classes.lower()
            if "time" not in classes_lower and not time_tag.has_attr("title"):
                continue
            title = time_tag.get("title")
            if title:
                return str(title).strip()
            text = time_tag.get_text(" ", strip=True)
            if text:
                return text

        text = container.get_text(" ", strip=True)
        match = re.search(r"\b\d{4}-\d{2}-\d{2}\b.*", text)
        if match:
            return match.group(0).strip()
        return ""

    def _extract_checkbox_value(self, row: Tag) -> Optional[str]:
        checkbox = row.find("input", attrs={"name": "conversations[]"})
        if checkbox and checkbox.get("value"):
            return str(checkbox.get("value"))
        return None

    def _find_inbox_rows(self, soup: BeautifulSoup) -> list[Tag]:
        form = soup.find("form", id="messageform")
        if not form:
            return []

        table = form.find("table")
        if not table:
            return []

        rows: list[Tag] = []
        for row in table.find_all("tr"):
            if row.find("input", attrs={"name": "conversations[]"}):
                rows.append(row)
        return rows

    def _find_staff_open_rows(self, soup: BeautifulSoup) -> list[Tag]:
        inbox = soup.find("div", id="inbox")
        if not inbox:
            return []

        open_heading = inbox.find("h3", string=lambda value: bool(value and value.strip().lower() == "open messages"))
        if not open_heading:
            return []

        open_table = open_heading.find_next("table")
        if not open_table:
            return []

        rows: list[Tag] = []
        for row in open_table.find_all("tr"):
            link = row.find("a", href=lambda href: bool(href and "staffpm.php?action=viewconv" in href))
            if link:
                rows.append(row)
        return rows

    def _extract_primary_link(self, container: Tag) -> Optional[str]:
        for link in container.find_all("a", href=True):
            href = str(link.get("href", ""))
            if any(keyword in href for keyword in ("notification", "staffpm", "inbox", "conversation", "torrents", "forum", "requests")):
                return href

        link = container.find("a", href=True)
        if link:
            return str(link.get("href", ""))
        return None

    def _build_message_url(self, base_url: str, message_id: str, is_staff: bool) -> str:
        fragment = f"#content{message_id}" if is_staff else f"#post{message_id}"
        return f"{base_url}{fragment}"

    def _is_unread_tag(self, tag: Tag) -> bool:
        classes = " ".join(tag.get("class", []))
        classes_lower = classes.lower()
        if any(token in classes_lower for token in ("unread", "highlight", "new", "urgent")):
            return True

        style = str(tag.get("style", "")).lower()
        if "font-weight" in style and "bold" in style:
            return True

        return bool(tag.find("strong"))

    def _extract_numeric_id(self, value: str) -> Optional[str]:
        match = re.search(r"(\d+)(?!.*\d)", value)
        if match:
            return match.group(1)
        return None

    def _extract_sender_from_header(self, header: Tag) -> str:
        header_copy = BeautifulSoup(str(header), "html.parser")
        post_id = header_copy.find("a", class_="post_id")
        if post_id:
            post_id.decompose()

        time_span = header_copy.find("span", class_="time")
        if time_span:
            time_span.decompose()

        for button in header_copy.find_all("button"):
            button.decompose()

        text = header_copy.get_text(" ", strip=True)
        text = re.sub(r"\s*-\s*$", "", text).strip()
        return text

    def _extract_form_token(self, html: str) -> str:
        soup = BeautifulSoup(html, "html.parser")
        token_input = soup.find("input", attrs={"name": "token"})
        if token_input and token_input.get("value"):
            return str(token_input.get("value"))
        return ""

    def _generate_otp_code(self) -> str:
        otp_uri = str(self.tracker_settings.get("otp_uri", "")).strip()
        if not otp_uri:
            return ""

        try:
            otp = pyotp.parse_uri(otp_uri)
        except Exception:
            log.error(f"{self.tracker}: Invalid otp_uri configured for MTV.")
            log.debug(f"{self.tracker}: otp_uri parse error details", exc_info=True)
            return ""

        if isinstance(otp, pyotp.TOTP):
            return otp.now()
        return ""

    def _persist_httpx_cookies(self, client: httpx.AsyncClient) -> None:
        self.cookie_jar.clear()
        for cookie in client.cookies.jar:
            self.cookie_jar.set_cookie(copy.copy(cookie))

        self.cookie_jar.save(ignore_discard=True, ignore_expires=True)

        self.client.cookies.clear()
        for cookie in self.cookie_jar:
            self.client.cookies.set(cookie.name, cookie.value, domain=cookie.domain, path=cookie.path)

    def _needs_relogin(self, response: str, success_text: str) -> bool:
        if not response:
            return False

        if self._looks_logged_in(response):
            if success_text and success_text not in response:
                return False
            return False

        response_lower = response.lower()
        return any(
            marker in response_lower
            for marker in (
                "unauthorized: insufficient authentication level",
                "<title>login",
                "name=\"token\"",
                "project luminance",
            )
        )

    def _looks_logged_in(self, response: str) -> bool:
        response_lower = response.lower()
        return "logout" in response_lower or "authkey=" in response_lower

    def _has_login_credentials(self) -> bool:
        username = str(self.tracker_settings.get("username", "")).strip()
        password = str(self.tracker_settings.get("password", "")).strip()
        return bool(username and password)