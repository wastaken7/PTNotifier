#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class PTCafe(NexusPHP):
    """
    Manages a session for PTCafe.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "PTCafe"
        base_url = "https://ptcafe.club/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
