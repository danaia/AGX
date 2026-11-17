"""Command dispatch for the native ANGI development swarm."""
import os
from pathlib import Path
import sys

from messaging import inbox, send
from session import launch, resume, status, dispatch
from ui import welcome


def main():
    # The private interpreter needs its own libraries at startup. Child tools
    # should use the live system libraries and their own wrapper environments.
    os.environ.pop("LD_LIBRARY_PATH", None)
    os.environ.pop("PYTHONHOME", None)
    args = sys.argv[1:]
    if args and args[0] == "_dispatch":
        dispatch(Path(args[1]))
        return
    if args and args[0] == "tell" and len(args) >= 3:
        send(args[1], " ".join(args[2:]))
        return
    if args == ["inbox"]:
        inbox()
        return
    if args == ["status"]:
        status()
        return
    if args != ["angi"]:
        print("Usage: agent angi | agent status | agent tell ROLE MESSAGE | agent inbox")
        return
    if os.getuid() == 0:
        raise RuntimeError("Run agent angi as your desktop account. Use angi-root for privileged work.")
    state = Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm"
    state.mkdir(parents=True, exist_ok=True, mode=0o700)
    action, brief = welcome((state / "current").exists())
    if action == "connect":
        os.execvp("opencode", ["opencode", "/var/lib/agx-data/angi-dev"])
    elif action == "resume":
        resume(state)
    elif action == "start":
        launch(state, brief)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, ValueError) as error:
        print(f"ANGI: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        sys.exit(130)
