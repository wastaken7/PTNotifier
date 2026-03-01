#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class BTSCHOOL(NexusPHP):
    """
    Manages a session for BTSCHOOL.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "BTSCHOOL"
        base_url = "https://pt.btschool.club/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
