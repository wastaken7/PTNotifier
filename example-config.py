from typing import Any

SETTINGS: dict[str, Any] = {
    "TELEGRAM_BOT_TOKEN": "",
    "TELEGRAM_CHAT_ID": "",
    "DISCORD_WEBHOOK_URL": "",
    # Seconds between checks
    # Make sure not to overload trackers
    # It might get you banned!
    # Minimum is 900 (15 minutes)
    "CHECK_INTERVAL": 1800,
    # Only available for some trackers
    "MARK_AS_READ": True,
    # HTTP request timeout in seconds
    # Default is 30 seconds
    # Increase if you have a slow connection or the tracker is slow
    "TIMEOUT": 30.0,
    # Minimum delay in seconds between requests
    # Default is 5 seconds
    "REQUEST_DELAY": 5.0,
}

API_TOKENS: dict[str, str] = {
    # Although Orpheus supports API, you still need to export cookies.
    "Orpheus": "",
}
