#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class March(NexusPHP):
    """
    Manages a session for March.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "March"
        base_url = "https://duckboobee.org/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
