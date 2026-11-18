"""Load the installed desktop release into the running session, as Settings > Refresh desktop... does.

The desktop refreshes itself when its own account sends it SIGUSR1: it saves settings, closes its terminals
and browser sessions, and execs /usr/bin/agx-gui again (the process keeps its PID). That closes the terminal
the update agent runs in, so the signal is sent by a detached watchdog a few seconds after the command
returns. The watchdog also guards the blind step: if the new release does not come up and stay up, it
restores the previous release so the supervisor starts a working desktop.
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

import releases

# Text compiled into every desktop that handles SIGUSR1. Any other desktop would be terminated by the signal.
MARKER = b"angi-refresh-on-sigusr1"
DELAY = 3       # seconds between the command returning and the signal, so the agent can finish its report
APPEAR = 30     # seconds for the new release to be running after the signal
STABLE = 20     # seconds it must then stay up
RESULT = Path(f"/var/lib/agx-data/agx-opencode-users/{os.getuid()}/refresh.json")


def desktops():
    """{pid: release name} of this account's running desktop processes."""
    found = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if (entry / "comm").read_text().strip() == "agx-gui" and entry.stat().st_uid == os.getuid():
                found[int(entry.name)] = Path(os.path.realpath(entry / "exe")).name
        except OSError:
            continue
    return found


def handles_refresh_signal(pid):
    try:
        return MARKER in Path(f"/proc/{pid}/exe").read_bytes()
    except OSError:
        return False


def report():
    """What the last automatic refresh did, or None."""
    try:
        return json.loads(RESULT.read_text())
    except (OSError, ValueError):
        return None


def finish(release, result, detail):
    RESULT.write_text(json.dumps({"release": release, "result": result, "detail": detail, "time": time.strftime("%Y-%m-%dT%H:%M:%S%z")}, indent=1) + "\n")
    return result


def start():
    """Schedule the refresh and return what to tell the user. Raises Refused when it must not or cannot happen."""
    current = releases.installed()
    if releases.entry(current) is None:
        raise releases.Refused(f"Release {current} has no source record; it was not installed by angi-update, so it is not refreshed automatically.")
    running = desktops()
    if not running:
        raise releases.Refused("No desktop session of this account is running, so there is nothing to refresh. The release loads when the desktop starts.")
    if len(running) > 1:
        raise releases.Refused("More than one desktop process is running. Ask the user to choose Settings > Refresh desktop... instead.")
    (pid, name), = running.items()
    if name == current:
        return f"The desktop already runs {current}; nothing to refresh."
    if not handles_refresh_signal(pid):
        raise releases.Refused(f"The running desktop ({name}) predates automatic refresh and would be terminated by the signal. "
                               "Ask the user to choose Settings > Refresh desktop... once; later releases can refresh themselves.")
    binary = releases.GUI_DIR / current
    with binary.open("rb") as program:
        elf = program.read(4) == b"\x7fELF"
    if not elf or not os.access(binary, os.X_OK):
        raise releases.Refused(f"Release {current} is not an executable program; not refreshing.")
    entry = releases.entry(current) or {}
    subprocess.Popen([sys.executable, str(Path(__file__).with_name("update.py")), "watch", str(pid), current, entry.get("parent") or ""],
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    return (f"Refreshing the desktop from {name} to {current} in {DELAY} seconds. This closes ANGI's terminals, including this one, "
            "so finish your report now. Reopen Agent Angie and resume to continue; `angi-update status` shows how the refresh went. "
            "If the new desktop does not stay up, the previous release is restored automatically.")


def watch(pid, release, previous, sleep=time.sleep, clock=time.monotonic):
    """Detached watchdog: signal the desktop, then make sure the new release came up and stayed up."""
    sleep(DELAY)
    try:
        os.kill(pid, signal.SIGUSR1)
    except OSError as error:
        return finish(release, "not-refreshed", f"Could not signal the desktop: {error}")
    deadline = clock() + APPEAR
    while desktops().get(pid) != release:
        if clock() >= deadline:
            if pid in desktops():
                return finish(release, "not-refreshed", "The desktop did not restart (settings could not be saved, or the signal was ignored). Choose Settings > Refresh desktop... instead.")
            return restore(release, previous, "The desktop was not running the new release after the refresh.")
        sleep(1)
    steady = clock() + STABLE
    while clock() < steady:
        if desktops().get(pid) != release:
            return restore(release, previous, "The new desktop stopped or restarted within seconds of loading.")
        sleep(1)
    return finish(release, "ok", "The new desktop is running.")


def restore(release, previous, why):
    try:
        releases.rollback()
        return finish(release, "rolled-back", f"{why} Restored {previous or 'the previous release'}.")
    except (releases.Refused, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        return finish(release, "failed", f"{why} Restoring the previous release also failed: {error}")
