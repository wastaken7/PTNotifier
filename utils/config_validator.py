from typing import Any

from utils.console import log


def load_config() -> tuple[dict[str, Any], dict[str, str], str | None, str | None, str | None, str | None, str | None, str | None, str | None]:
    """
    Load configuration from config.py.

    Returns:
        tuple: A tuple containing:
            - user_config (dict[str, Any]): User configuration settings.
            - api_tokens (dict[str, str]): API tokens for trackers.
            - discord_webhook_url (str | None): Discord webhook URL.
            - telegram_bot_token (str | None): Telegram bot token.
            - telegram_chat_id (str | None): Telegram chat ID.
            - gotify_url (str | None): Gotify URL.
            - gotify_token (str | None): Gotify token.
            - ntfy_url (str | None): Ntfy URL.
            - ntfy_topic (str | None): Ntfy topic.
    """
    try:
        import config as _imported_config
    except ImportError:
        log.warning("config.py not found. Creating from example-config.py...")
        log.warning("Please edit config.py with your settings before running again.")
        import shutil

        shutil.copyfile("example-config.py", "config.py")
        exit(1)

    try:
        user_config: dict[str, Any] = _imported_config.SETTINGS
        api_tokens: dict[str, str] = _imported_config.API_TOKENS
        telegram_bot_token = user_config.get("TELEGRAM_BOT_TOKEN")
        telegram_chat_id = user_config.get("TELEGRAM_CHAT_ID")
        discord_webhook_url = user_config.get("DISCORD_WEBHOOK_URL")
        gotify_url = user_config.get("GOTIFY_URL")
        gotify_token = user_config.get("GOTIFY_TOKEN")
        ntfy_url = user_config.get("NTFY_URL")
        ntfy_topic = user_config.get("NTFY_TOPIC")
        if "IGNORE_STRING" not in user_config:
            user_config["IGNORE_STRING"] = {}

    except Exception as e:
        log.error(f"Error loading config.py: {e}")
        log.debug("Config error details", exc_info=True)
        log.error("Check example-config.py for any missing fields.")
        exit(1)

    has_telegram = telegram_bot_token and telegram_chat_id
    has_discord = discord_webhook_url
    has_gotify = gotify_url and gotify_token
    has_ntfy = ntfy_url and ntfy_topic

    if not has_telegram and not has_discord and not has_gotify and not has_ntfy:
        log.error("Please set TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID, DISCORD_WEBHOOK_URL, GOTIFY_URL and GOTIFY_TOKEN, or NTFY_URL and NTFY_TOPIC in config.py")
        exit(1)

    return user_config, api_tokens, discord_webhook_url, telegram_bot_token, telegram_chat_id, gotify_url, gotify_token, ntfy_url, ntfy_topic

