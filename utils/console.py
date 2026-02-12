import logging
import random
import sys

from rich.console import Console
from rich.logging import RichHandler
from rich.text import Text

console = Console()

logging.getLogger("httpx").setLevel(logging.WARNING)
logging.getLogger("h2").setLevel(logging.WARNING)
logging.getLogger("hpache").setLevel(logging.WARNING)
logging.getLogger("httpcore").setLevel(logging.WARNING)
logging.getLogger("asyncio").setLevel(logging.WARNING)
logging.getLogger("urllib3").setLevel(logging.WARNING)

clean_handler = RichHandler(
    show_time=False,
    show_level=False,
    show_path=False,
    markup=True,
)

level_logging = logging.DEBUG if "-debug" in sys.argv or "--debug" in sys.argv else logging.INFO
logging.basicConfig(
    level=level_logging,
    format="%(message)s",
    handlers=[clean_handler],
)
log = logging.getLogger("rich")

ascii_lines = [
    " ███████████  ███████████ ██████   █████           █████     ███     ██████   ███",
    "░░███░░░░░███░█░░░███░░░█░░██████ ░░███           ░░███     ░░░     ███░░███ ░░░",
    " ░███    ░███░   ░███  ░  ░███░███ ░███   ██████  ███████   ████   ░███ ░░░  ████   ██████  ████████",
    " ░██████████     ░███     ░███░░███░███  ███░░███░░░███░   ░░███  ███████   ░░███  ███░░███░░███░░███",
    " ░███░░░░░░      ░███     ░███ ░░██████ ░███ ░███  ░███     ░███ ░░░███░     ░███ ░███████  ░███ ░░░",
    " ░███            ░███     ░███  ░░█████ ░███ ░███  ░███ ███ ░███   ░███      ░███ ░███░░░   ░███",
    " █████           █████    █████  ░░█████░░██████   ░░█████  █████  █████     █████░░██████  █████",
    "░░░░░           ░░░░░    ░░░░░    ░░░░░  ░░░░░░     ░░░░░  ░░░░░  ░░░░░     ░░░░░  ░░░░░░  ░░░░░",
]


def get_random_rgb():
    """Generates a random RGB tuple."""
    return (random.randint(0, 255), random.randint(0, 255), random.randint(0, 255))


color_start = get_random_rgb()
color_end = get_random_rgb()


def interpolate_color(c1: tuple[int, int, int], c2: tuple[int, int, int], factor: float):
    """Calculates a color between c1 and c2 based on the factor (0.0 to 1.0)."""
    r = int(c1[0] + (c2[0] - c1[0]) * factor)
    g = int(c1[1] + (c2[1] - c1[1]) * factor)
    b = int(c1[2] + (c2[2] - c1[2]) * factor)
    return f"rgb({r},{g},{b})"


for line in ascii_lines:
    text = Text()
    line_length = len(line)

    for i, char in enumerate(line):
        if char in "█░":
            factor = i / line_length if line_length > 1 else 0
            current_color = interpolate_color(color_start, color_end, factor)
            text.append(char, style=current_color)
        else:
            text.append(char)

    console.print(text, soft_wrap=True)
