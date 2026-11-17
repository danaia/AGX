"""What a build run may touch: the ANGI operating system, or one project folder the user chose.

The scope is instructions in each agent's prompt plus the folder the panes start in. It steers the
agents; it is not a sandbox, because the agents run as the desktop account with its permissions.
"""
import os
from pathlib import Path

OS_SOURCE = Path("/var/lib/agx-data/angi-dev")
DATA = Path("/var/lib/agx-data")
# Under the data volume only the user's own folders qualify; these hold the system and credentials.
# `users/<name>` is the person's own NVMe space, so it is allowed; ownership keeps others' out.
INTERNAL = ("angi-sdk", "bin", "lost+found", "tailscale", "herdr-preview", "herdr-setup.log")


def validate_project(path):
    """The resolved project folder, or ValueError saying what to change. The only check that counts."""
    text = str(path).strip()
    if not text:
        raise ValueError("Enter a folder path.")
    expanded = Path(text).expanduser()
    if not expanded.is_absolute():
        raise ValueError("Enter the full path of the folder, starting with /.")
    try:
        folder = expanded.resolve(strict=True)
    except (OSError, RuntimeError):
        raise ValueError("That folder does not exist.")
    if not folder.is_dir():
        raise ValueError("That is a file, not a folder.")
    home = Path.home().resolve()
    inside = lambda parent: folder == parent or parent in folder.parents
    if folder == OS_SOURCE or inside(OS_SOURCE):
        raise ValueError("That is ANGI's own source. Use OS mode to extend the operating system.")
    if not (inside(home) or inside(DATA)):
        raise ValueError("Choose a folder in your home or on the data volume.")
    if folder == home or folder == DATA:
        raise ValueError("Choose a project folder inside it, not the whole location.")
    if inside(DATA) and not inside(home):
        top = folder.relative_to(DATA).parts[0]
        if top.startswith(("agx-", "angi-")) or top in INTERNAL:
            raise ValueError("That folder is part of the system. Choose one of your own.")
    if any(part.startswith(".") for part in folder.relative_to(home if inside(home) else DATA).parts):
        raise ValueError("Choose a folder that is not hidden or a settings folder.")
    status = folder.stat()
    if status.st_uid != os.getuid() or not os.access(folder, os.R_OK | os.W_OK | os.X_OK):
        raise ValueError("You need to own the folder and be able to change it.")
    return folder


def describe(project):
    """Name and path shown for a validated folder."""
    return {"path": str(project), "name": project.name}


OS_SCOPE = """Scope: the ANGI operating system.
Work only on the operating system of this device: its source in {source}, packages, services,
desktop and system configuration. Do not create or modify unrelated projects or the user's personal
files. Use angi-build for native builds. The user has authorized administrator access through
angi-root for this operating-system work; use it when an installation needs root."""

PROJECT_SCOPE = """Scope: the user's project at {project}.
Work only inside that folder: read, create, edit, build and run things there, and keep the run's
files (code, tests, notes) in it. Do not modify the ANGI operating system, its source checkout at
{source}, services, system files or anyone else's files. The angi-development skill describes this
device's toolchain; use it to build and run the project, and ignore its source-checkout, deployment
and root-install guidance. Do not use angi-root; if the project needs a system change, stop and ask
the user."""


def scope_text(project=None):
    if project is None:
        return OS_SCOPE.format(source=OS_SOURCE)
    return PROJECT_SCOPE.format(project=project, source=OS_SOURCE)
