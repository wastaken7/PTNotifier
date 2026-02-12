import importlib
import pkgutil
from pathlib import Path
from typing import Any

from utils.console import log


def load_trackers() -> dict[str, Any]:
    """Dynamically loads all tracker classes from the 'trackers' directory."""
    log.info("Loading trackers...")
    trackers: dict[str, Any] = {}
    tracker_modules = pkgutil.iter_modules([str(Path("trackers"))])
    for tracker_info in tracker_modules:
        if not tracker_info.ispkg:
            try:
                module_name = tracker_info.name
                module = importlib.import_module(f"trackers.{module_name}")
                tracker_class_name = f"{module_name}"
                if hasattr(module, tracker_class_name):
                    tracker_class = getattr(module, tracker_class_name)
                    if hasattr(tracker_class, "fetch_notifications"):
                        trackers[module_name] = tracker_class
                    else:
                        log.error(f"Tracker class {tracker_class_name} does not have a fetch_notifications method.")
                else:
                    if module_name != "base":
                        log.error(f"Tracker module {module_name} does not have a class named {tracker_class_name}.")
            except Exception as e:
                log.error(f"Failed to load tracker {tracker_info.name}:", exc_info=e)
    return trackers
