#!/usr/bin/env python3

import json
from abc import ABC, abstractmethod
from collections.abc import Coroutine
from http.cookiejar import MozillaCookieJar
from pathlib import Path
from typing import Any, Callable

import httpx
from bs4 import BeautifulSoup
from rich.console import Console

console = Console()


class BaseTracker(ABC):
    """
    Base class for tracker sessions.
    """

    def __init__(self, cookie_path: Path, tracker_name: str, base_url: str):
        self.tracker = tracker_name
        self.cookie_path = cookie_path
        self.filename = cookie_path.name
        self.base_url = base_url

        self.state_path = Path("./state") / f"{self.tracker}.json"
        self.first_run = False
        self.state: dict[str, Any] = self._load_state()

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

        self.client = httpx.AsyncClient(
            headers=self.headers,
            cookies=self.cookie_jar,
            timeout=30.0,
            follow_redirects=True,
            http2=True,
        )

    def _load_state(self) -> dict[str, Any]:
        if self.state_path.exists():
            try:
                return json.loads(self.state_path.read_text("utf-8"))
            except Exception:
                return {"processed_ids": []}
        else:
            console.print(f"{self.tracker}: No existing state file found. There won't be any notifications on the first run.")
            self.first_run = True
        return {"processed_ids": []}

    def _save_state(self):
        self.state_path.parent.mkdir(parents=True, exist_ok=True)
        self.state_path.write_text(
            json.dumps(self.state, ensure_ascii=False, indent=2), "utf-8"
        )

    def _ack_item(self, item_id: str):
        if item_id not in self.state["processed_ids"]:
            self.state["processed_ids"].append(item_id)
            if len(self.state["processed_ids"]) > 300:
                self.state["processed_ids"] = self.state["processed_ids"][-300:]
            self._save_state()

    @abstractmethod
    async def process(
        self,
        send_telegram: Callable[[dict[str, Any], str, str], Coroutine[Any, Any, None]],
    ):
        """Main loop to fetch and process notifications."""
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
            console.print(f"Error reading domain from {cookie_path.name}: {e}")
        return ""

    async def _fetch_page(self, url: str) -> BeautifulSoup | None:
        """Fetches a page and returns a BeautifulSoup object."""
        try:
            response = await self.client.get(url)
            response.raise_for_status()
            return BeautifulSoup(response.text, "html.parser")
        except httpx.HTTPStatusError as e:
            console.print(
                f"{self.tracker}: HTTP error fetching {url}: {e.response.status_code} - {e.response.text}"
            )
        except Exception as e:
            console.print(f"{self.tracker}: Error fetching {url}: {e}")
        return None
