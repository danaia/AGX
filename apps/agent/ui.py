"""Small curses build-brief form; no web server or extra UI runtime."""
import curses
import textwrap


def welcome(can_resume):
    return curses.wrapper(_form, can_resume)


def _form(screen, can_resume):
    screen.keypad(True)
    brief = ""
    editing = False
    while True:
        screen.erase()
        height, width = screen.getmaxyx()
        def line(y, text, style=0):
            if 0 <= y < height - 1:
                screen.addnstr(y, 2, text, max(0, width - 4), style)
        line(1, "ANGI / Agent workshop", curses.A_BOLD)
        line(3, "OS updates: one fast agent   ·   Projects: orchestrator / builder / tester")
        line(4, "Native Pi builds · NVMe workspace · administrator tools")
        if editing:
            line(6, "What would you like to build?", curses.A_BOLD)
            for index, chunk in enumerate(textwrap.wrap(brief or " ", max(1, width - 6))[-max(1, height - 11):]):
                line(8 + index, chunk)
            line(height - 2, "Enter: start    Esc: back    Backspace: edit")
        else:
            line(7, "N  Start a new build", curses.A_BOLD)
            if can_resume:
                line(9, "R  Resume the last run")
            line(11, "C  Open OpenCode to connect a model provider (/connect)")
            line(13, "Q  Back to terminal")
            line(height - 3, "Project runs use Herdr: Ctrl+B then Q detaches; agent angi resumes.")
            line(height - 2, "Source: /var/lib/agx-data/angi-dev")
        screen.refresh()
        key = screen.get_wch()
        if editing:
            if key in ("\n", "\r") and brief.strip():
                return "start", brief.strip()
            if key == "\x1b":
                editing = False
            elif key in (curses.KEY_BACKSPACE, "\x7f", "\b"):
                brief = brief[:-1]
            elif isinstance(key, str) and key.isprintable() and len(brief) < 8000:
                brief += key
        elif isinstance(key, str):
            key = key.lower()
            if key == "n": editing = True
            elif key == "r" and can_resume: return "resume", ""
            elif key == "c": return "connect", ""
            elif key in ("q", "\x1b"): return "quit", ""
