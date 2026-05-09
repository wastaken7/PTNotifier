#!/usr/bin/env python3

from pathlib import Path

from trackers.NexusPHP import NexusPHP


class PTGTK(NexusPHP):
    """
    Manages a session for PTGTK.
    """

    def __init__(self, cookie_path: Path):
        tracker_name = "PTGTK"
        base_url = "https://pt.gtkpw.xyz/"
        super().__init__(cookie_path=cookie_path, tracker_name=tracker_name, base_url=base_url)
