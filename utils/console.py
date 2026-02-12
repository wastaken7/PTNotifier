import logging

from rich.console import Console
from rich.logging import RichHandler

console = Console()

logging.getLogger("httpx").setLevel(logging.WARNING)
logging.getLogger("httpcore").setLevel(logging.WARNING)
clean_handler = RichHandler(
    show_time=False,
    show_level=False,
    show_path=False,
    markup=True,
)
logging.basicConfig(
    level=logging.INFO,
    format="%(message)s",
    handlers=[clean_handler],
)
log = logging.getLogger("rich")

ASCII_ART = r"""
[#00f0ff]  ____    ______  __  __          __           ___						 [/#00f0ff]
[#22c8ff] /\  _ \ /\__  _\/\ \/\ \        /\ \__  __  / ___\ __					 [/#22c8ff]
[#44a0ff] \ \ \_\ \/_/\ \/\ \ `\\ \    ___\ \  _\/\_\/\ \__//\_\     __   _ __	 [/#44a0ff]
[#6678ff]  \ \  __/  \ \ \ \ \   ` \  / __`\ \ \/\/\ \ \  __\/\ \  /'__`\/\`'__\ [/#6678ff]
[#8850ff]   \ \ \/    \ \ \ \ \ \`\ \/\ \_\ \ \ \_\ \ \ \ \_/\ \ \/\  __/\ \ \/	 [/#8850ff]
[#aa28ff]    \ \_\     \ \_\ \ \_\ \_\ \____/\ \__\\ \_\ \_\  \ \_\ \____\\ \_\	 [/#aa28ff]
[#cc00ff]     \/_/      \/_/  \/_/\/_/\/___/  \/__/ \/_/\/_/   \/_/\/____/ \/_/	 [/#cc00ff]
"""
console.print(ASCII_ART)
