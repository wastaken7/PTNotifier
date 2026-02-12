
from utils.console import log


def valid_response(tracker: str, response: str, keyword: str) -> bool:
    if keyword and keyword not in response:
        log.error(
            f"{tracker}: [bold red]Request failed. The cookie appears to be expired/invalid, or the site is currently down or the HTML structure has changed. Please log in through your usual browser and export the cookies again. Keyword '{keyword}' not found.[/bold red]"
        )
        return False
    return True
