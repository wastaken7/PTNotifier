import subprocess

import httpx

from utils.console import log


async def check_version():
    """Checks for new versions of the script on GitHub.s"""
    try:
        log.info("Checking for new version...")
        local_hash = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()  # noqa: ASYNC221

        async with httpx.AsyncClient() as client:
            resp = await client.get("https://api.github.com/repos/wastaken7/PTNotifier/commits/main")
            resp.raise_for_status()
            remote_hash = resp.json()["sha"]

        if local_hash != remote_hash:
            log.warning("A new update is available on main branch.")
            log.info("Run 'git pull' to stay up to date.\n")
        else:
            log.info("No new updates found.")

    except Exception as e:
        log.error("Version check failed:", exc_info=e)
        log.warning("Make sure git is installed and you have internet connectivity.")
