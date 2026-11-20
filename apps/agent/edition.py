"""Runtime capabilities set by the installed board image."""
from pathlib import Path

BOARD = Path("/usr/share/agx/board")


def zero2w():
    try:
        return BOARD.read_text().strip() == "zero2w"
    except FileNotFoundError:
        return False
