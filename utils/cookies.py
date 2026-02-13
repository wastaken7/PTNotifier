from pathlib import Path

from utils.console import log


def valid_response(tracker: str, response: str, keyword: str) -> bool:
    """
    Checks if the response contains a specific keyword to validate the session.

    Args:
        tracker (str): The name of the tracker being checked.
        response (str): The HTML content of the response.
        keyword (str): The keyword to search for in the response.

    Returns:
        bool: True if the keyword is found or if no keyword is provided, False otherwise.
    """
    if keyword and keyword not in response:
        log.error(
            f"{tracker}: [bold red]Request failed. The cookie appears to be expired/invalid, or the site is currently down or the HTML structure has changed. Please log in through your usual browser and export the cookies again. Keyword '{keyword}' not found.[/bold red]"
        )
        save_html(tracker, response)
        return False
    return True

def save_html(tracker: str, html_content: str) -> None:
    """
    Saves the provided HTML content to a file for debugging purposes.

    Args:
        tracker (str): The name of the tracker being checked.
        html_content (str): The HTML content to be saved.

    Returns:
        None
    """
    try:
        debug_path = Path("./debug")
        debug_path.mkdir(parents=True, exist_ok=True)
        file_path = debug_path / f"{tracker}_debug.html"
        file_path.write_text(html_content, encoding="utf-8")
        log.debug(f"{tracker}: HTML saved to {file_path}")
    except Exception as e:
        log.error(f"{tracker}: Failed to save HTML debug file:", exc_info=e)
