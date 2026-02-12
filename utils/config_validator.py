from typing import Any

from utils.console import log


def load_config() -> tuple[dict[str, Any], dict[str, str], str | None, str | None, str | None]:
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

    except Exception as e:
        log.error("Error loading config.py:", exc_info=e)
        log.error("Check example-config.py for any missing fields.")
        exit(1)

    if (not telegram_bot_token or not telegram_chat_id) and not discord_webhook_url:
        log.error("Please set TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID or DISCORD_WEBHOOK_URL in config.py")
        exit(1)

    return user_config, api_tokens, discord_webhook_url, telegram_bot_token, telegram_chat_id

