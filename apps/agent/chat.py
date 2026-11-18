"""Chat: one OpenCode agent with Angie's persona, no Herdr, no project folder, no tools.

Chat is for talking. The agent loads the angi-chat skill, may use nothing else, and starts in its own
private folder so resuming continues the chat and never the OS agent's session.
"""
import json
import os
from pathlib import Path
import subprocess
import time

from models import active_selections
from scope import chat_scope_text
import session

AGENT = "angi-chat"
SKILL = "angi-chat"
MARKER = "chat-current"


def config():
    """OpenCode configuration with the single chat agent. It may load skills and nothing else."""
    profile = {"description": "Angie, in conversation", "mode": "primary", "permission": {"*": "deny", "skill": "allow"},
               "prompt": (session.ASSETS / "profiles/chat.md").read_text() + "\n" + chat_scope_text()}
    # Chat is a single agent like an OS update, so it shares that role's model; none chosen means OpenCode's default.
    model = active_selections().get("os")
    if model:
        profile["model"] = model
    return session.opencode_config({AGENT: profile})


def prepare(state):
    session.install_skills()
    os.environ["OPENCODE_CONFIG_CONTENT"] = config()
    home = state / "chat"
    home.mkdir(mode=0o700, exist_ok=True)
    os.chdir(home)


def launch(state, brief):
    prepare(state)
    run = state / ("angi-chat-" + time.strftime("%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    run.mkdir(mode=0o700)
    (run / "run.json").write_text(json.dumps({"mode": "chat", "solo": True, "brief": brief}, indent=2))
    marker = state / f"{MARKER}.{os.getpid()}.new"
    marker.write_text(run.name)
    marker.replace(state / MARKER)
    os.execvp("opencode", ["opencode", "--agent", AGENT, "--prompt", brief])


def resume(state):
    prepare(state)
    os.execvp("opencode", ["opencode", "--agent", AGENT, "--continue"])


def parse_reply(output):
    """(text, error) from `opencode run --format json` output: the agent's text parts, or its error message."""
    text, error = [], None
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("type") == "text":
            text.append(event.get("part", {}).get("text", ""))
        elif event.get("type") == "error":
            detail = event.get("error", {})
            error = (detail.get("data") or {}).get("message") or detail.get("message") or detail.get("name") or "Angie hit a problem."
    return "".join(text).strip(), error


def turn(state, message, new=False, timeout=300):
    """One chat exchange for the desktop's chat view: {"text": reply} or {"error": reason}.

    A new chat starts a fresh OpenCode session; otherwise the folder's last session continues.
    """
    prepare(state)
    command = ["opencode", "run", "--agent", AGENT, "--format", "json", *([] if new else ["--continue"]), message]
    try:
        result = subprocess.run(command, capture_output=True, text=True, stdin=subprocess.DEVNULL, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"error": "Angie took too long to answer. Try again."}
    except OSError:
        return {"error": "OpenCode is not available."}
    reply, error = parse_reply(result.stdout)
    if reply:
        return {"text": reply}
    lines = result.stderr.strip().splitlines()
    return {"error": error or (lines[-1] if lines else "Angie did not answer. Check the model in Agent Angie's provider setup.")}


def newest(state):
    """True when the last run was a chat, not an OS update or a swarm."""
    chat = state / MARKER
    if not chat.exists():
        return False
    return all(not other.exists() or chat.stat().st_mtime >= other.stat().st_mtime for other in (state / "os-current", state / "current"))
