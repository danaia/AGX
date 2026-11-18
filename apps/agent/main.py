"""Command dispatch for the native ANGI development swarm."""
import json
import os
from pathlib import Path
import sys

from messaging import inbox, send
import chat
import solo
from session import attach, launch, resume, status, dispatch
from ui import welcome
from models import activate, active, catalog, save, selections
from scope import describe, validate_project


def angi_request(args):
    """(brief, project, chat) from `angi`, `angi --prompt BRIEF`, `... --project DIR` or `... --chat`; False if malformed."""
    if args == ["angi"]:
        return None, None, False
    if args[:2] == ["angi", "--prompt"] and len(args) in (3, 4, 5) and args[2].strip():
        if len(args) == 3:
            return args[2].strip(), None, False
        if len(args) == 4 and args[3] == "--chat":
            return args[2].strip(), None, True
        if len(args) == 5 and args[3] == "--project" and args[4].strip():
            return args[2].strip(), args[4], False
    return False


def main():
    # The private interpreter needs its own libraries at startup. Child tools
    # should use the live system libraries and their own wrapper environments.
    os.environ.pop("LD_LIBRARY_PATH", None)
    os.environ.pop("PYTHONHOME", None)
    args = sys.argv[1:]
    if args == ["provider-bridge"]:
        from providers import bridge
        bridge()
        return
    if args == ["models"]:
        print(json.dumps(selections()))
        return
    if len(args) == 2 and args[0] == "models":
        print(json.dumps(catalog(args[1])))
        return
    if args == ["provider"]:
        print(json.dumps(active()))
        return
    if len(args) == 2 and args[0] == "provider":
        print(json.dumps(activate(args[1])))
        return
    if len(args) == 3 and args[0] == "model":
        print(json.dumps(save(args[1], args[2])))
        return
    if len(args) == 2 and args[0] == "project":
        # The prompt asks this before it enables Start; launch checks again.
        try:
            print(json.dumps(describe(validate_project(args[1]))))
        except ValueError as error:
            print(json.dumps({"error": str(error)}))
        return
    if len(args) in (2, 3) and args[0] == "chat-turn" and (len(args) == 2 or args[1] == "--new") and args[-1].strip():
        # One exchange for the desktop's chat bubbles: a single JSON line, text or error.
        if os.getuid() == 0:
            raise RuntimeError("Run agent chat-turn as your desktop account.")
        state = Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm"
        state.mkdir(parents=True, exist_ok=True, mode=0o700)
        print(json.dumps(chat.turn(state, args[-1].strip(), new=len(args) == 3)))
        return
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
    if args == ["attach"]:
        if os.getuid() == 0:
            raise RuntimeError("Run agent attach as your desktop account.")
        attach(Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm")
        return
    request = angi_request(args)
    if request is False:
        print("Usage: agent angi [--prompt BRIEF [--project DIR | --chat]] | agent attach | agent status | agent tell ROLE MESSAGE | agent inbox")
        return
    brief, project, talk = request
    if os.getuid() == 0:
        raise RuntimeError("Run agent angi as your desktop account. Use angi-root for privileged work.")
    state = Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm"
    state.mkdir(parents=True, exist_ok=True, mode=0o700)
    action, brief = ("start", brief) if brief else welcome((state / "current").exists() or (state / "os-current").exists() or (state / chat.MARKER).exists())
    if action == "connect":
        os.execvp("opencode", ["opencode", "/var/lib/agx-data/angi-dev"])
    elif action == "resume":
        # The most recent run decides: a chat or an OS update run is one agent, a project run is a Herdr swarm.
        (chat.resume if chat.newest(state) else solo.resume if solo.newest(state) else resume)(state)
    elif action == "start":
        # Chat and OS updates are single agents (OS installs through angi-update); only projects use Herdr.
        if talk:
            chat.launch(state, brief)
        else:
            solo.launch(state, brief) if project is None else launch(state, brief, project)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, ValueError) as error:
        print(f"ANGI: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        sys.exit(130)
