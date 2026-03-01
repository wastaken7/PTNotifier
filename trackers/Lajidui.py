#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class Lajidui(NexusPHP):
    """
    Manages a session for Lajidui.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "Lajidui"
        base_url = "https://pt.lajidui.top/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
