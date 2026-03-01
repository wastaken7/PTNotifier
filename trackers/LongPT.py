#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class LongPT(NexusPHP):
    """
    Manages a session for LongPT.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "LongPT"
        base_url = "https://longpt.org/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
