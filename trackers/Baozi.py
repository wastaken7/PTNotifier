#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class Baozi(NexusPHP):
    """
    Manages a session for Baozi.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "Baozi"
        base_url = "https://p.t-baozi.cc/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
