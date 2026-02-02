#!/usr/bin/env python3

import asyncio
import json
import time
from abc import ABC, abstractmethod
from collections.abc import Coroutine
from http.cookiejar import MozillaCookieJar
from pathlib import Path
from typing import Any, Callable, Optional

import httpx
from bs4 import BeautifulSoup
from rich.console import Console

import config

console = Console()


class BaseTracker(ABC):
    """
    Base class for tracker sessions.
    """

    _request_lock = asyncio.Lock()
    _last_request_time = 0.0

    def __init__(self, cookie_path: Path, tracker_name: str, base_url: str, custom_headers: Optional[dict[str, str]] = None):
        if custom_headers is None:
            custom_headers = {}
        self.tracker = tracker_name
        self.cookie_path = cookie_path
        self.filename = cookie_path.name
        self.base_url = base_url

        self.state_path = Path("./state") / f"{self.tracker}.json"
        self.first_run = False
        self.last_request_time: float = 0.0
        self.state: dict[str, Any] = self._load_state()

        try:
            min_run_interval = float(str(config.SETTINGS.get("CHECK_INTERVAL", 1800.0)))
        except (ValueError, TypeError):
            min_run_interval = 1800.0

        self.should_run = time.time() - self.state.get("last_run", 0) >= min_run_interval

        self.cookie_jar = MozillaCookieJar(self.cookie_path)
        try:
            self.cookie_jar.load(ignore_discard=True, ignore_expires=True)
        except Exception as e:
            console.print(
                f"{self.tracker}: Failed to load cookies from {self.filename}: {e}"
            )

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
            console.print(f"{self.tracker}: [yellow]No existing state file found.[/yellow] There won't be any notifications on the first run.")
            self.first_run = True
            self.state = {"processed_ids": [], "last_run": 0}
            self._save_state()
            return self.state

    def _save_state(self):
        try:
            self.state_path.parent.mkdir(parents=True, exist_ok=True)
            self.state_path.write_text(json.dumps(self.state, ensure_ascii=False, indent=2), "utf-8")
        except Exception as e:
            console.print(f"{self.tracker}: [bold red]Error saving state:[/bold red] {e}")

    async def _ack_item(self, item: dict[str, Any]) -> None:
        """Marks an item as processed."""
        item_id = str(item["id"])
        if item_id not in self.state["processed_ids"]:
            self.state["processed_ids"].append(item_id)
            if len(self.state["processed_ids"]) > 300:
                self.state["processed_ids"] = self.state["processed_ids"][-300:]
            self._save_state()

    async def fetch_notifications(self, send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]]):
        if self.should_run:
            self.state["last_run"] = time.time()
            self._save_state()
            await self.process(send_telegram)
        else:
            console.print(f"{self.tracker}: Skipping check, last run was too recent.")

    async def process(
        self,
        send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]],
    ) -> None:
        """Main loop to fetch and process notifications."""
        try:
            all_items = await self._fetch_items()

            for item in all_items:
                if not self.first_run:
                    await send_telegram(item, self.tracker, item["url"])
                    await asyncio.sleep(3)
                await self._ack_item(item)

        except Exception as e:
            console.print(f"{self.tracker}: [bold red]Error processing {self.base_url}:[/bold red] {e}")
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
            console.print(f"[bold red]Error reading domain from {cookie_path.name}:[/bold red] {e}")
        return ""

    async def _fetch_page(self, url: str, request_type: str) -> Optional[BeautifulSoup]:
        try:
            delay = float(str(config.SETTINGS.get("REQUEST_DELAY", 5.0)))
            timeout = float(str(config.SETTINGS.get("TIMEOUT", 30.0)))
        except (ValueError, TypeError):
            delay, timeout = 5.0, 30.0

        async with BaseTracker._request_lock:
            current_time = time.monotonic()
            elapsed = current_time - BaseTracker._last_request_time

            if elapsed < delay:
                sleep_time = delay - elapsed
                await asyncio.sleep(sleep_time)

            BaseTracker._last_request_time = time.monotonic()

            try:
                console.print(f"{self.tracker}: [blue]Checking for {request_type}...[/blue]")
                response = await self.client.get(url, timeout=timeout)
                response.raise_for_status()
                return BeautifulSoup(response.text, "html.parser")
            except httpx.HTTPStatusError as e:
                console.print(f"{self.tracker}: [bold red]HTTP error [/bold red]{e.response.status_code}")
            except Exception as e:
                console.print(f"{self.tracker}: [bold red]Error: [/bold red]{e}")

            return None
