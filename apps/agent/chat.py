"""Chat: one OpenCode agent with optional tools for a selected local folder."""
import json
import os
from pathlib import Path
import subprocess
import time

from models import active_selections
from scope import chat_scope_text, validate_project
import session

AGENT = "angi-chat"
SKILL = "angi-chat"
MARKER = "chat-current"


def config(folder):
    """Include Angi's persona in the prompt so it is available on every provider."""
    skill = (session.ASSETS / "skills" / SKILL / "SKILL.md").read_text().split("---", 2)[2].strip()
    prompt = (session.ASSETS / "profiles/chat.md").read_text().strip()
    from chat_files import TOOLS
    permission = {"*": "deny", **{f"angi_files_{name}": "allow" for name in TOOLS}}
    profile = {"description": "Angi, in conversation", "mode": "primary", "permission": permission,
               "prompt": "\n\n".join((prompt, chat_scope_text(folder), skill))}
    # Chat is a single agent like an OS update, so it shares that role's model; none chosen means OpenCode's default.
    model = active_selections().get("os")
    if model:
        profile["model"] = model
    result = json.loads(session.opencode_config({AGENT: profile}))
    result["mcp"] = {"angi_files": {
        "type": "local",
        "command": ["/usr/bin/python3", str(Path(__file__).with_name("chat_files.py").resolve()), str(folder)],
    }}
    return json.dumps(result)


def prepare(state, folder=None):
    if folder is None:
        folder = str(Path.home().resolve())
    selected = validate_project(folder, allow_home=True)
    if str(selected) != folder:
        raise ValueError("The Chat folder has moved. Select it again.")
    session.start_size_guard()
    os.environ["OPENCODE_CONFIG_CONTENT"] = config(selected)
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
            error = (detail.get("data") or {}).get("message") or detail.get("message") or detail.get("name") or "Angi hit a problem."
    return "".join(text).strip(), error


def turn(state, message, new=False, timeout=300, folder=None):
    """One chat exchange for the desktop's chat view: {"text": reply} or {"error": reason}.

    A new chat starts a fresh OpenCode session; otherwise the folder's last session continues.
    """
    command = ["opencode", "run", "--agent", AGENT, "--format", "json", *([] if new else ["--continue"]), "--", message]
    try:
        prepare(state, folder)
    except (OSError, RuntimeError, ValueError) as error:
        return {"error": str(error)}
    try:
        result = subprocess.run(command, capture_output=True, text=True, stdin=subprocess.DEVNULL, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"error": "Angi took too long to answer. Try again."}
    except OSError:
        return {"error": "OpenCode is not available."}
    reply, error = parse_reply(result.stdout)
    if reply:
        return {"text": reply}
    lines = result.stderr.strip().splitlines()
    return {"error": error or (lines[-1] if lines else "Angi did not answer. Check the model in Agent Angi's provider setup.")}


def newest(state):
    """True when the last run was a chat, not an OS update or a swarm."""
    chat = state / MARKER
    if not chat.exists():
        return False
    return all(not other.exists() or chat.stat().st_mtime >= other.stat().st_mtime for other in (state / "os-current", state / "current"))
