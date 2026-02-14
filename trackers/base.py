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
from apps.telegram import send_telegram
from utils.console import log
from utils.cookies import valid_response


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
        self.state: dict[str, Any] = self._load_state()
        self.first_run = False
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
        """
        tracker_name = tracker_name.replace("https://", "").replace("http://", "")
        if "." in tracker_name:
            tracker_name = tracker_name.split(".")[0]
            tracker_name = tracker_name.capitalize()
        return tracker_name

    def get_scrape_interval(self, scrape_interval: float) -> float:
        """
        Returns the scrape interval, ensuring it is not lower than the global setting.
        """
        config_interval = float(str(config.SETTINGS.get("SCRAPE_INTERVAL", 1800)))
        if scrape_interval >= config_interval:
            return scrape_interval
        else:
            return config_interval

    def _load_state(self) -> dict[str, Any]:
        if self.state_path.exists():
            try:
                state = json.loads(self.state_path.read_text("utf-8"))
                if "processed_ids" not in state or "last_run" not in state:
                    raise ValueError("State is missing required keys")
                return state
            except Exception:
                return {"processed_ids": [], "last_run": 0}
        else:
            log.warning(f"{self.tracker}: No existing state file found. There won't be any notifications on the first run to avoid spamming.")
            self.first_run = True
            self.state = {"processed_ids": [], "last_run": 0}
            self._save_state()
            return self.state

    def _save_state(self):
        try:
            self.state_path.parent.mkdir(parents=True, exist_ok=True)
            self.state_path.write_text(json.dumps(self.state, ensure_ascii=False, indent=2), "utf-8")
        except Exception as e:
            log.error(f"{self.tracker}: Error saving state: {e}")
            log.debug("State error details", exc_info=True)

    async def _ack_item(self, item: dict[str, Any]) -> None:
        """Marks an item as processed."""
        item_id = str(item["id"])
        if item_id not in self.state["processed_ids"]:
            self.state["processed_ids"].append(item_id)
            if len(self.state["processed_ids"]) > 300:
                self.state["processed_ids"] = self.state["processed_ids"][-300:]

    async def fetch_notifications(self) -> float:
        if time.time() - self.state.get("last_run", 0) >= self.scrape_interval:
            await self.process()
            return self.scrape_interval
        else:
            remaining_time = self.state.get("last_run", 0) + self.scrape_interval - time.time()
            if remaining_time > 0:
                log.debug(f"{self.tracker}: Skipping check, next run in {remaining_time / 60:.2f} minutes.")
            return remaining_time

    async def process(self) -> None:
        """Main loop to fetch and process notifications."""
        notifiers: list[Callable[[dict[str, Any], str, str, str], Coroutine[Any, Any, None]]] = []
        telegram_bot_token = config.SETTINGS.get("TELEGRAM_BOT_TOKEN")
        telegram_chat_id = config.SETTINGS.get("TELEGRAM_CHAT_ID")
        discord_webhook_url = config.SETTINGS.get("DISCORD_WEBHOOK_URL")

        if telegram_bot_token and telegram_chat_id:
            notifiers.append(send_telegram)
        if discord_webhook_url:
            notifiers.append(send_discord)

        try:
            all_items: list[dict[str, Any]] = await self._fetch_items()

            for item in all_items:
                if not self.first_run:
                    for notifier in notifiers:
                        await notifier(
                            item,
                            self.tracker,
                            self.base_url,
                            item["url"],
                        )
                        await asyncio.sleep(3)
                await self._ack_item(item)
            self.state["last_run"] = time.time()
            self._save_state()
        except Exception as e:
            log.error(f"{self.tracker}: Error processing {self.base_url}: {e}")
            log.debug("Processing error details", exc_info=True)
        finally:
            await self.client.aclose()

    @abstractmethod
    async def _fetch_items(self) -> list[dict[str, Any]]:
        """Fetch all new items from the tracker."""
        raise NotImplementedError

    @staticmethod
    def _extract_domain_from_cookie(cookie_path: Path) -> str:
        """Reads the first valid domain from the Netscape cookie file."""
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

        :param url: The URL to fetch.
        :param request_type: A descriptive name for the request (used for logging).
        :param success_text: A keyword to look for in the response to verify a successful login/session.
        :return: The response text.
        :raises RequestError: If the request fails or validation fails.
        :raises ValueError: If inputs are invalid.
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
                valid_response(self.tracker, response.text, success_text)

            async with BaseTracker._request_lock:
                BaseTracker._last_request_time = time.monotonic()

            log.debug(f"{self.tracker}: Successfully fetched {request_type}")
            return response.text

        except httpx.HTTPStatusError as e:
            error_msg = f"{self.tracker}: HTTP {e.response.status_code} error for {request_type}"
            log.error(error_msg)

        except httpx.TimeoutException:
            error_msg = f"{self.tracker}: Timeout fetching {request_type}"
            log.error(error_msg)

        except httpx.RequestError as e:
            error_msg = f"{self.tracker}: Network error fetching {request_type}: {e}"
            log.error(error_msg)

        except Exception:
            error_msg = f"{self.tracker}: Unexpected error fetching {request_type}"
            log.error(error_msg)
            log.debug("Error details", exc_info=True)

        return ""

