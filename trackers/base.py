#!/usr/bin/env python3

import asyncio
import json
import time
from abc import ABC, abstractmethod
from collections.abc import Callable, Coroutine
from http.cookiejar import MozillaCookieJar
from pathlib import Path
from typing import Any, Optional

import httpx

import config
from apps.discord import send_discord
from apps.gotify import send_gotify
from apps.ntfy import send_ntfy
from apps.telegram import send_telegram
from utils.console import log
from utils.cookies import valid_response


class TrackerRequestError(Exception):
    """
    Raised when a request to a tracker fails due to connection issues,
    timeout, HTTP error status, or failed validation/expired cookies.
    """

    pass


class BaseTracker(ABC):
    """
    Base class for tracker sessions.
    """

    _request_lock = asyncio.Lock()
    _last_request_time = 0.0

    def __init__(
        self,
        cookie_path: Path,
        tracker_name: str,
        base_url: str,
        custom_headers: Optional[dict[str, str]] = None,
        scrape_interval: float = 1800,
    ):
        if custom_headers is None:
            custom_headers = {}
        self.tracker = self.get_tracker_name(tracker_name)
        self.scrape_interval = self.get_scrape_interval(scrape_interval)
        self.cookie_path = cookie_path
        self.filename = cookie_path.name
        self.cookie_jar = MozillaCookieJar(self.cookie_path)
        self.base_url = base_url
        self.state_path = Path("./state") / f"{self.tracker}.json"
        self.first_run = False
        self.state: dict[str, Any] = self._load_state()
        if not getattr(self, "api_only", False):
            try:
                self.cookie_jar.load(ignore_discard=True, ignore_expires=True)
            except Exception as e:
                log.error(f"{self.tracker}: Failed to load cookies from {self.filename}: {e}")
                log.debug("Cookie error details", exc_info=True)

        self.headers = {
            "User-Agent": "PTNotifier 1.0 (https://github.com/wastaken7/PTNotifier)",
        }
        if custom_headers:
            self.headers.update(custom_headers)

        self.client = httpx.AsyncClient(
            headers=self.headers,
            cookies=self.cookie_jar,
            timeout=30.0,
            follow_redirects=True,
            http2=True,
        )
        self.request_lock = asyncio.Lock()

    def get_tracker_name(self, tracker_name: str) -> str:
        """
        Returns a clean tracker name from the provided string.

        Args:
            tracker_name (str): The tracker name to clean.

        Returns:
            str: The cleaned tracker name.
        """
        tracker_name = tracker_name.replace("https://", "").replace("http://", "")
        if "." in tracker_name:
            tracker_name = tracker_name.split(".")[0]
            tracker_name = tracker_name.capitalize()
        return tracker_name

    def get_scrape_interval(self, scrape_interval: float) -> float:
        """
        Returns the scrape interval, ensuring it is not lower than the global setting.

        Args:
            scrape_interval (float): The scrape interval to use.

        Returns:
            float: The scrape interval, or the global setting if it is lower.
        """
        config_interval = float(str(config.SETTINGS.get("SCRAPE_INTERVAL", 1800)))
        if scrape_interval >= config_interval:
            return scrape_interval
        else:
            return config_interval

    def _load_state(self) -> dict[str, Any]:
        """
        Load state from the state file.

        Returns:
            dict[str, Any]: The state, or an empty state if the file doesn't exist or is invalid.
        """
        if self.state_path.exists():
            try:
                state = json.loads(self.state_path.read_text("utf-8"))
                if "processed_ids" not in state or "last_run" not in state:
                    raise ValueError("State is missing required keys")
                return state
            except Exception:
                return {"processed_ids": [], "last_run": 0}
        else:
            log.warning(
                f"{self.tracker}: No existing state file found. "
                "There won't be any notifications on the first run to avoid spamming."
            )
            self.first_run = True
            self.state = {"processed_ids": [], "last_run": 0}
            self._save_state()
            return self.state

    def _save_state(self):
        """
        Save state to the state file.
        """
        try:
            self.state_path.parent.mkdir(parents=True, exist_ok=True)
            self.state_path.write_text(json.dumps(self.state, ensure_ascii=False, indent=2), "utf-8")
        except Exception as e:
            log.error(f"{self.tracker}: Error saving state: {e}")
            log.debug("State error details", exc_info=True)

    async def _ack_item(self, item: dict[str, Any]) -> None:
        """
        Marks an item as processed.

        Args:
            item (dict[str, Any]): The item to mark as processed.
        """
        item_id = str(item["id"])
        if item_id not in self.state["processed_ids"]:
            self.state["processed_ids"].append(item_id)
            if len(self.state["processed_ids"]) > 300:
                self.state["processed_ids"] = self.state["processed_ids"][-300:]

    def _collect_notifiers(
        self,
    ) -> list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]]:
        """
        Collects all enabled notifiers from the configuration.

        Returns:
            list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]]: List of notifier functions.
        """
        notifiers: list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]] = []
        telegram_bot_token = config.SETTINGS.get("TELEGRAM_BOT_TOKEN")
        telegram_chat_id = config.SETTINGS.get("TELEGRAM_CHAT_ID")
        discord_webhook_url = config.SETTINGS.get("DISCORD_WEBHOOK_URL")
        gotify_url = config.SETTINGS.get("GOTIFY_URL")
        gotify_token = config.SETTINGS.get("GOTIFY_TOKEN")
        ntfy_url = config.SETTINGS.get("NTFY_URL")
        ntfy_topic = config.SETTINGS.get("NTFY_TOPIC")

        if telegram_bot_token and telegram_chat_id:
            notifiers.append(send_telegram)
        if discord_webhook_url:
            notifiers.append(send_discord)
        if gotify_url and gotify_token:
            notifiers.append(send_gotify)
        if ntfy_url and ntfy_topic:
            notifiers.append(send_ntfy)
        return notifiers

    async def _send_item_notifications(
        self,
        item: dict[str, Any],
        notifiers: list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]],
    ) -> None:
        """
        Sends the item to all enabled notifiers.

        Args:
            item (dict[str, Any]): The item to send.
            notifiers (list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]]): List of notifier functions.
        """
        for notifier in notifiers:
            await notifier(
                item,
                self.tracker,
                self.base_url,
                item["url"],
            )
            await asyncio.sleep(3)

    async def send_error_notification(self, error_message: str) -> None:
        """
        Sends an error notification to all configured notifiers.
        Respects the 24-hour rate limit (one per site per 24 hours).
        """
        if not config.SETTINGS.get("SEND_ERROR_NOTIFICATIONS", False):
            log.info(f"{self.tracker}: Error notification suppressed (disabled by config).")
            return

        now = time.time()
        last_error_time = self.state.get("last_error_notification_time", 0.0)

        # 24 hours in seconds = 86400
        if now - last_error_time < 86400:
            log.info(f"{self.tracker}: Error notification suppressed (sent in the last 24h).")
            return

        notifiers = self._collect_notifiers()
        if not notifiers:
            log.warning(f"{self.tracker}: No notification backends configured, skipping error notification.")
            return

        body = (
            f"An error occurred while fetching notifications from {self.tracker} ({self.base_url}).\n\n"
            f"**Error:** {error_message}\n\n"
            "Error messages are sent only once every 24 hours per tracker."
        )

        item = {
            "type": "error",
            "title": "Request Failure / Expired Cookies",
            "subject": f"Error communicating with {self.tracker}",
            "body": body,
            "date": time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(now)),
            "url": self.base_url,
        }

        log.info(f"{self.tracker}: Sending error notification to active notifiers...")

        # Send the notification to all active notifiers
        for notifier in notifiers:
            try:
                await notifier(
                    item,
                    self.tracker,
                    self.base_url,
                    item["url"],
                )
                await asyncio.sleep(3)
            except Exception as notifier_err:
                log.error(f"{self.tracker}: Failed to send error notification via notifier: {notifier_err}")

        # Update and save the state with the timestamp
        self.state["last_error_notification_time"] = now
        self._save_state()

    async def fetch_notifications(self) -> float:
        """
        Fetch notifications from the tracker, respecting the scrape interval.

        Returns:
            float: The time until the next run, or 0 if the tracker was just processed.
        """
        if time.time() - self.state.get("last_run", 0) >= self.scrape_interval:
            await self.process()
            return self.scrape_interval
        else:
            remaining_time = self.state.get("last_run", 0) + self.scrape_interval - time.time()
            if remaining_time > 0:
                log.debug(f"{self.tracker}: Skipping check, next run in {remaining_time / 60:.2f} minutes.")
            return remaining_time

    async def send_test_notification(self) -> bool:
        """
        Send one test notification without updating tracker state.

        Returns:
            bool: True if the test notification was sent successfully, False otherwise.
        """
        notifiers = self._collect_notifiers()
        if not notifiers:
            log.error(f"{self.tracker}: No notification backends are configured.")
            await self.client.aclose()
            return False

        try:
            log.info(f"{self.tracker}: Preparing test item...")
            item = await self._fetch_test_item()
            if not item:
                log.warning(f"{self.tracker}: No message or notification available for test delivery.")
                return False

            # Tracker-specific test-item implementations may bypass the base
            # fetch helpers and return a BeautifulSoup Tag as the message body.
            # Normalize it before passing the item to notification backends,
            # just as the regular processing path does.
            self._post_process_item(item)

            log.info(f"{self.tracker}: Sending test item '{item.get('subject') or item.get('title') or item.get('id')}'...")
            await self._send_item_notifications(item, notifiers)
            log.info(f"{self.tracker}: Test notification sent.")
            return True
        except Exception as e:
            log.error(f"{self.tracker}: Error sending test notification: {e}")
            log.debug("Test notification error details", exc_info=True)
            return False
        finally:
            await self.client.aclose()

    async def process(self) -> None:
        """
        Main loop to fetch and process notifications.
        """
        notifiers = self._collect_notifiers()

        try:
            all_items: list[dict[str, Any]] = await self._fetch_items()
            for item in all_items:
                self._post_process_item(item)

            for item in all_items:
                if not self.first_run:
                    if self._should_ignore(item):
                        log.info(f"{self.tracker}: Ignoring notification due to keyword filter.")
                        await self._ack_item(item)
                        continue
                    await self._send_item_notifications(item, notifiers)
                await self._ack_item(item)
            self.state["last_run"] = time.time()
            self._save_state()
            self.save_cookies()
        except Exception as e:
            log.error(f"{self.tracker}: Error processing {self.base_url}: {e}")
            log.debug("Processing error details", exc_info=True)
            await self.send_error_notification(str(e))
        finally:
            await self.client.aclose()

    def _clean_html(self, element: Any) -> str:
        """
        Converts a BeautifulSoup element (Tag) into a clean HTML string
        where <br> tags are replaced with newlines, relative urls are made absolute,
        and only formatting-related HTML tags are preserved.
        """
        if element is None:
            return ""

        from urllib.parse import urljoin

        def parse_node(node: Any) -> str:
            if getattr(node, "name", None) is None:
                return str(node)

            tag_name = node.name.lower()
            if tag_name == "br":
                return "\n"

            children_text = "".join(parse_node(child) for child in node.children)

            if tag_name in ("p", "div", "li", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol"):
                return f"\n{children_text}\n"

            if tag_name == "a":
                href = node.get("href", "")
                if href:
                    absolute_href = urljoin(self.base_url, href)
                    return f'<a href="{absolute_href}">{children_text}</a>'
                return children_text

            elif tag_name in ("b", "strong", "i", "em", "u", "s", "strike", "spoiler", "tg-spoiler", "code"):
                return f"<{tag_name}>{children_text}</{tag_name}>"

            return children_text

        raw_result = "".join(parse_node(child) for child in element.children)

        # Normalize whitespace and newlines
        cleaned_lines = [line.strip() for line in raw_result.split("\n")]
        import re

        result = "\n".join(cleaned_lines)
        result = re.sub(r"\n{3,}", "\n\n", result)

        return result.strip()

    def _post_process_item(self, item: dict[str, Any]) -> None:
        """
        Post-processes a fetched item, converting any BeautifulSoup Tag or HTML
        string inside the 'body' field into a clean, formatted text string.
        """
        if "body" in item:
            body = item["body"]
            if body is not None and not isinstance(body, str):
                item["body"] = self._clean_html(body)
            elif isinstance(body, str) and ("<" in body and ">" in body):
                from bs4 import BeautifulSoup

                soup = BeautifulSoup(body, "html.parser")
                item["body"] = self._clean_html(soup)

    def _should_ignore(self, item: dict[str, Any]) -> bool:
        """
        Checks if the item contains any ignore keywords for this tracker.

        Args:
            item (dict[str, Any]): The item to check.

        Returns:
            bool: True if the item should be ignored, False otherwise.
        """
        ignore_config: dict[str, list[str]] = config.SETTINGS.get("IGNORE_STRING", {})

        # Check for both base_url with and without trailing slash
        url_with_slash = self.base_url if self.base_url.endswith("/") else self.base_url + "/"
        url_no_slash = self.base_url.rstrip("/")

        keywords = ignore_config.get(url_with_slash) or ignore_config.get(url_no_slash)

        if not keywords:
            return False

        # Fields to check
        fields_to_check = [item.get("title"), item.get("subject"), item.get("body")]

        return any(field and any(keyword.lower() in str(field).lower() for keyword in keywords) for field in fields_to_check)

    @abstractmethod
    async def _fetch_items(self) -> list[dict[str, Any]]:
        """
        Fetch all new items from the tracker.

        Returns:
            list[dict[str, Any]]: List of items.
        """
        raise NotImplementedError

    async def _fetch_test_item(self) -> Optional[dict[str, Any]]:
        """
        Fetch one test item from the tracker.

        Returns:
            Optional[dict[str, Any]]: Test item, or None if not found.
        """
        items = await self._fetch_items()
        if items:
            for item in items:
                self._post_process_item(item)
            return items[0]

        return await self._fetch_processed_test_item()

    async def _fetch_processed_test_item(self) -> Optional[dict[str, Any]]:
        """
        Fetch one test item from the tracker, including processed items.

        Returns:
            Optional[dict[str, Any]]: Test item, or None if not found.
        """
        original_processed_ids = list(self.state.get("processed_ids", []))

        try:
            self.state["processed_ids"] = [""]
            items = await self._fetch_items()
            for item in items:
                self._post_process_item(item)
        finally:
            self.state["processed_ids"] = original_processed_ids

        for item in items:
            if item.get("type") == "message":
                return item

        return None

    def save_cookies(self) -> None:
        """
        Saves updated cookies to the cookie file.
        """
        if not getattr(self, "api_only", False):
            try:
                self.cookie_jar.save(ignore_discard=True, ignore_expires=True)
            except Exception as e:
                log.error(f"{self.tracker}: Failed to save updated cookies to {self.filename}: {e}")

    @staticmethod
    def _extract_domain_from_cookie(cookie_path: Path) -> str:
        """
        Reads the first valid domain from the Netscape cookie file.

        Args:
            cookie_path (Path): The path to the cookie file.

        Returns:
            str: The first valid domain found in the cookie file, or an empty string if not found.
        """
        try:
            with open(cookie_path, encoding="utf-8") as f:
                for line in f:
                    if line.strip() and not line.startswith("#"):
                        parts = line.split("\t")
                        if len(parts) > 0:
                            domain = parts[0].lstrip(".")
                            if "." in domain:
                                return domain
        except Exception as e:
            log.error(f"Error reading domain from {cookie_path.name}: {e}")
            log.debug("Cookie error details", exc_info=True)
        return ""

    async def _fetch_page(
        self,
        url: str,
        request_type: str,
        success_text: str = "",
    ) -> str:
        """
        Fetches a page with a global rate limit and optional validation.

        Args:
            url (str): The URL to fetch.
            request_type (str): A descriptive name for the request (used for logging).
            success_text (str): A keyword to look for in the response to verify a successful login/session.

        Returns:
            str: The response text.

        Raises:
            RequestError: If the request fails or validation fails.
            ValueError: If inputs are invalid.
        """
        try:
            delay = float(config.SETTINGS.get("REQUEST_DELAY", 5.0))
            timeout = float(config.SETTINGS.get("TIMEOUT", 30.0))

            if delay < 0 or timeout < 0:
                raise ValueError("Delay and timeout must be positive")

        except (ValueError, TypeError) as e:
            log.warning(f"Invalid config values, using defaults: {e}")
            delay, timeout = 5.0, 30.0

        async with BaseTracker._request_lock:
            current_time = time.monotonic()
            elapsed = current_time - BaseTracker._last_request_time

            if elapsed < delay:
                sleep_time = delay - elapsed
                log.debug(f"{self.tracker}: Rate limiting - sleeping {sleep_time:.2f}s")
                await asyncio.sleep(sleep_time)

        try:
            log.debug(f"{self.tracker}: Fetching {request_type} from {url}")
            response = await self.client.get(url, timeout=timeout)
            response.raise_for_status()

            if success_text:
                if not valid_response(self.tracker, response.text, success_text):
                    raise TrackerRequestError(
                        f"Validation failed: keyword '{success_text}' not found in the HTML response. "
                        "Possible reasons: Expired cookies, IP ban, site maintenance, HTML change."
                    )

            async with BaseTracker._request_lock:
                BaseTracker._last_request_time = time.monotonic()

            log.debug(f"{self.tracker}: Successfully fetched {request_type}")
            self.save_cookies()
            return response.text

        except httpx.HTTPStatusError as e:
            error_msg = f"{self.tracker}: HTTP {e.response.status_code} error for {request_type}"
            log.error(error_msg)
            raise TrackerRequestError(error_msg) from e

        except httpx.TimeoutException as e:
            error_msg = f"{self.tracker}: Timeout fetching {request_type}"
            log.error(error_msg)
            raise TrackerRequestError(error_msg) from e

        except httpx.RequestError as e:
            error_msg = f"{self.tracker}: Network error fetching {request_type}: {e}"
            log.error(error_msg)
            raise TrackerRequestError(error_msg) from e

        except TrackerRequestError:
            raise

        except Exception as e:
            error_msg = f"{self.tracker}: Unexpected error fetching {request_type}"
            log.error(error_msg)
            log.debug("Error details", exc_info=True)
            raise TrackerRequestError(f"{error_msg}: {e}") from e
