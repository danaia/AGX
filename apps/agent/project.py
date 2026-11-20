"""Run one OpenCode coding agent in a user-selected project."""
import os
from pathlib import Path
import time

from models import active_selections, atomic_write
from scope import validate_project
from session import opencode_config

AGENT = "angi-project"


def config(folder):
    profile = {
        "description": "ANGI Project coding",
        "mode": "primary",
        "permission": {"*": "ask", "task": "deny"},
        "prompt": (
            f"Work on the user's task inside their selected project folder, {folder}. "
            "Keep code, notes, and tests in that folder. Leave system files and unrelated projects alone. "
            "Make focused changes and verify the result before reporting back."
        ),
    }
    if model := active_selections().get("builder"):
        profile["model"] = model
    return opencode_config({AGENT: profile})


def launch(state, brief, folder):
    project = validate_project(folder)
    if str(project) != folder:
        raise ValueError("The Project folder has moved. Select it again.")
    settings = config(project)
    run = state / (time.strftime("angi-project-%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    run.mkdir(mode=0o700)
    atomic_write(run / "run.json", {"mode": "project", "solo": True, "brief": brief, "project": str(project)})
    os.environ["OPENCODE_CONFIG_CONTENT"] = settings
    os.chdir(project)
    os.execvp("opencode", ["opencode", "--agent", AGENT, "--prompt", brief])
