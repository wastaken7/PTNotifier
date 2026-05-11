from typing import Any

SETTINGS: dict[str, Any] = {
    "TELEGRAM_BOT_TOKEN": "",
    "TELEGRAM_CHAT_ID": "",
    "DISCORD_WEBHOOK_URL": "",
    # Discord embed description limit. Discord currently allows up to 4096 characters.
    "DISCORD_EMBED_DESCRIPTION_LIMIT": 4096,
    "GOTIFY_URL": "",
    "GOTIFY_TOKEN": "",
    "NTFY_URL": "https://ntfy.sh",
    "NTFY_TOPIC": "",
    "NTFY_TOKEN": "",
    "NTFY_PRIORITY": 3,
    # Seconds between checks
    # Make sure not to overload trackers
    # It might get you banned!
    # Minimum is 1800 (30 minutes)
    "CHECK_INTERVAL": 1800,
    # Only available for some trackers
    "MARK_AS_READ": False,
    # HTTP request timeout in seconds
    # Default is 30 seconds
    # Increase if you have a slow connection or the tracker is slow
    "TIMEOUT": 30.0,
    # Minimum delay in seconds between requests
    # Default is 5 seconds
    "REQUEST_DELAY": 5.0,
    # Optional: Specify strings that will cause a notification to be ignored.
    # The key is the tracker's base URL and the value is a list of strings to ignore.
    # The check is case-insensitive and matches anywhere in the notification.
    # Example: If you don't want to be notified about "torrent deleted" or "your class has changed",
    # you can add them to the ignore list like this:
    "IGNORE_STRING": {
        "https://example.com/": ["torrent deleted", "your class has changed"],
    },
}

API_TOKENS: dict[str, str] = {
    # Although Orpheus supports API, you still need to export cookies.
    "Orpheus": "",
    # Only "User" permission is required.
    "Anthelion": "",
}
