#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class PTFans(NexusPHP):
    """
    Manages a session for PTFans.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "PTFans"
        base_url = "https://ptfans.cc/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
