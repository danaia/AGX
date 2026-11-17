"""Tailscale state, sign-in and the LM Studio relay target (userspace mode; see apps/tailscale)."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

TAILSCALE = "/usr/bin/tailscale"
SOCKET = "/run/tailscale/tailscaled.sock"
# A loopback relay (agx-tailscale) carries this port to the LM Studio address chosen below.
RELAY = "http://127.0.0.1:1234/v1"
TARGET = Path("/var/lib/agx-data/tailscale/config/lmstudio-target")
LOGIN = re.compile(r"https://login\.tailscale\.com/\S+")


def run(*arguments, timeout=15):
    return subprocess.run([TAILSCALE, "--socket", SOCKET, *arguments], capture_output=True, text=True, timeout=timeout)


def state():
    """What the settings panel shows: whether LM Studio can be reached, and whether sign-in is offered."""
    if not Path(TAILSCALE).exists():
        return {"ready": False, "login": False, "text": "Tailscale is not installed on this device. Run scripts/deploy-tailscale.sh from the ANGI source."}
    try:
        data = json.loads(run("status", "--json", timeout=8).stdout)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return {"ready": False, "login": False, "text": "Tailscale is not running. It starts with the device; restart it from Services."}
    backend = data.get("BackendState")
    if backend == "Running":
        name = ((data.get("Self") or {}).get("DNSName") or "").rstrip(".")
        return {"ready": True, "login": False, "text": f"Connected to Tailscale as {name or 'this device'}."}
    if backend == "NeedsLogin":
        return {"ready": False, "login": True, "text": "Sign in to Tailscale so ANGI can reach LM Studio privately."}
    if backend == "Stopped":
        return {"ready": False, "login": True, "text": "Tailscale is turned off. Turn it on to reach LM Studio."}
    return {"ready": False, "login": False, "text": "Tailscale is starting…"}


def login():
    """Start sign-in as the desktop user. Returns the page to approve it on, or None if no sign-in is needed."""
    try:
        result = run("up", "--timeout=10s", timeout=25)
    except subprocess.TimeoutExpired:
        raise RuntimeError("Tailscale did not respond. Try again.")
    except OSError:
        raise RuntimeError("Tailscale is not installed on this device.")
    found = LOGIN.search(result.stdout + result.stderr)
    if found:
        return found.group(0)
    try:
        auth = json.loads(run("status", "--json", timeout=8).stdout).get("AuthURL")
    except (OSError, ValueError, subprocess.TimeoutExpired):
        auth = None
    if auth and auth.startswith("https://login.tailscale.com/"):
        return auth
    return None


def target():
    """The saved LM Studio host and port, or None."""
    try:
        host, port = TARGET.read_text().split()
        return host, int(port)
    except (OSError, ValueError):
        return None


def set_target(host, port):
    """Replace the relay target atomically; the relay re-reads it for every connection."""
    TARGET.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".target.", dir=TARGET.parent)
    try:
        with os.fdopen(descriptor, "w") as output:
            output.write(f"{host} {port}\n")
        os.chmod(temporary, 0o664)
        os.replace(temporary, TARGET)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
