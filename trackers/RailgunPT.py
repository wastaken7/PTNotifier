#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class RailgunPT(NexusPHP):
    """
    Manages a session for RailgunPT.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "RailgunPT"
        base_url = "https://bilibili.download/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
