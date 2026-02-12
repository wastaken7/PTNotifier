import subprocess
import time

import httpx
from rich.panel import Panel

from utils.console import console, log

last_check_time = 0.0

async def check_version():
    """
    Checks for new commits on GitHub by comparing the local HEAD with the remote main branch.
    Displays a list of all missing commit messages in a Rich panel.
    """
    global last_check_time

    current_time = time.time()
    if current_time - last_check_time < 86400:
        return
    last_check_time = current_time

    try:
        log.debug("Checking for new version...")

        local_hash = subprocess.run(  # noqa: ASYNC221
            ["git", "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()

        compare_url = f"https://api.github.com/repos/wastaken7/PTNotifier/compare/{local_hash}...main"

        async with httpx.AsyncClient() as client:
            response = await client.get(compare_url)
            response.raise_for_status()
            data = response.json()

        commits = data.get("commits", [])

        if commits:
            message = "[yellow]A new update is available on the main branch.[/yellow]\n"
            message += f"\n[bold white]Pending Changes ({len(commits)}):[/bold white]\n"

            for commit in commits:
                commit_msg = commit["commit"]["message"].split("\n")[0]
                message += f"• [italic]{commit_msg}[/italic]\n"

            message += "\n[cyan]Run 'git pull' to stay up to date.[/cyan]"

            console.print(Panel(message, title="Update Available", border_style="bold green", expand=False))
        else:
            log.debug("No new updates found. You are up to date.")

    except subprocess.CalledProcessError:
        log.error("Failed to retrieve local git hash. Is git initialized?")
    except httpx.HTTPStatusError as e:
        if e.response.status_code == 404:
            log.warning("Local hash not found on remote. History might have diverged.")
        else:
            log.error(f"GitHub API error: {e}")
    except Exception as e:
        log.error("Version check failed:", exc_info=e)
        log.warning("Ensure you have an active internet connection.")
