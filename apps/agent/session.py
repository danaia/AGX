"""Herdr session lifecycle and queued peer delivery."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from messaging import ROLES, database

SOURCE = Path("/var/lib/agx-data/angi-dev")
ASSETS = Path("/usr/share/agx/agent")


def herdr(session, *arguments, timeout=45):
    result = subprocess.run(["herdr", "--session", session, *arguments], capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    return json.loads(result.stdout)["result"]


def configure():
    config = Path(os.environ["HERDR_CONFIG_PATH"])
    config.parent.mkdir(parents=True, exist_ok=True)
    if not config.exists():
        config.write_text((ASSETS / "herdr.toml").read_text())
    # Herdr 0.9.5 hard-codes ~/.config/opencode for installation. Give only
    # that installer a private HOME pointing back to OpenCode's real XDG store.
    opencode_config = Path(os.environ["XDG_CONFIG_HOME"]) / "opencode"
    opencode_config.mkdir(parents=True, exist_ok=True)
    installer_home = Path(os.environ["TMPDIR"]) / "herdr-installer"
    (installer_home / ".config").mkdir(parents=True, exist_ok=True)
    link = installer_home / ".config/opencode"
    if not link.is_symlink():
        link.symlink_to(opencode_config, target_is_directory=True)
    result = subprocess.run(["herdr", "integration", "install", "opencode"], env={**os.environ, "HOME": str(installer_home)}, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    skill = Path(os.environ["XDG_CONFIG_HOME"]) / "opencode/skills/angi-development"
    skill.mkdir(parents=True, exist_ok=True)
    (skill / "SKILL.md").write_text((ASSETS / "skills/angi-development/SKILL.md").read_text())


def start_server(name, run):
    try:
        herdr(name, "workspace", "list", timeout=3)
        return
    except (RuntimeError, subprocess.TimeoutExpired):
        pass
    with (run / "server.log").open("a") as log:
        subprocess.Popen(["herdr", "--session", name, "server"], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    for _ in range(100):
        try:
            herdr(name, "workspace", "list", timeout=2)
            return
        except (RuntimeError, subprocess.TimeoutExpired):
            time.sleep(.1)
    raise RuntimeError(f"Herdr did not start. Inspect {run / 'server.log'}")


def role_config():
    profiles = {}
    for role in ROLES:
        profiles[f"angi-{role}"] = {
            "description": f"ANGI {role}", "mode": "primary",
            "prompt": (ASSETS / f"profiles/{role}.md").read_text(),
            "permission": {"*": "allow", "task": "deny"},
        }
    return json.dumps({"agent": profiles, "share": "disabled", "autoupdate": False})


def launch(state, brief):
    if not (SOURCE / "Cargo.toml").is_file():
        raise RuntimeError("Native source checkout is missing. Install the ANGI development SDK first.")
    configure()
    name = "angi-" + time.strftime("%Y%m%d-%H%M%S") + f"-{os.getpid()}"
    run = state / name
    run.mkdir(mode=0o700)
    (run / "brief.txt").write_text(brief + "\n")
    os.environ["OPENCODE_CONFIG_CONTENT"] = role_config()
    start_server(name, run)
    workspace = herdr(name, "workspace", "create", "--cwd", str(SOURCE), "--label", "ANGI workshop", "--env", f"AGX_SWARM_RUN={run}", "--env", "AGX_SWARM_ROLE=orchestrator", "--env", "OPENCODE_CONFIG_CONTENT=" + role_config(), "--no-focus")
    root = workspace["root_pane"]["pane_id"]
    # Split panes inherit the server environment, not their parent terminal's
    # overrides. Carry the run explicitly so every peer can address its mailbox.
    right = herdr(name, "pane", "split", root, "--direction", "right", "--ratio", "0.5", "--cwd", str(SOURCE), "--env", f"AGX_SWARM_RUN={run}", "--env", "AGX_SWARM_ROLE=builder", "--no-focus")["pane"]["pane_id"]
    bottom = herdr(name, "pane", "split", right, "--direction", "down", "--cwd", str(SOURCE), "--env", f"AGX_SWARM_RUN={run}", "--env", "AGX_SWARM_ROLE=tester", "--no-focus")["pane"]["pane_id"]
    manifest = {"session": name, "panes": dict(zip(ROLES, (root, right, bottom))), "brief": brief}
    (run / "run.json").write_text(json.dumps(manifest, indent=2))
    temporary = state / f"current.{os.getpid()}.new"
    temporary.write_text(name)
    temporary.replace(state / "current")
    # Save the run before starting agents, so startup/provider failures remain
    # visible and can be resumed without creating another set of processes.
    ensure_dispatcher(run)
    for role, pane in manifest["panes"].items():
        herdr(name, "pane", "rename", pane, role.title())
        task = f"The user wants: {brief}" if role == "orchestrator" else "Read the angi-development skill, report readiness to orchestrator with agent tell, then wait for an assigned task."
        print(f"Starting ANGI {role}…", flush=True)
        try:
            # OpenCode owns its startup submission; injecting terminal input
            # during its first paint can lose the brief on a cold Pi startup.
            herdr(name, "agent", "start", role, "--kind", "opencode", "--pane", pane, "--timeout", "60000", "--", "--agent", f"angi-{role}", "--prompt", task, timeout=65)
        except (RuntimeError, subprocess.TimeoutExpired) as error:
            print(f"{role}: {error}", file=sys.stderr)
    herdr(name, "workspace", "focus", workspace["workspace"]["workspace_id"])
    os.execvp("herdr", ["herdr", "--session", name])


def selected(state):
    name = (state / "current").read_text().strip()
    if not name.startswith("angi-") or Path(name).name != name:
        raise RuntimeError("Invalid saved swarm.")
    return state / name


def resume(state):
    configure()
    os.environ["OPENCODE_CONFIG_CONTENT"] = role_config()
    run = selected(state)
    name = json.loads((run / "run.json").read_text())["session"]
    start_server(name, run)
    ensure_dispatcher(run)
    os.execvp("herdr", ["herdr", "--session", name])


def status():
    state = Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm"
    run = selected(state)
    manifest = json.loads((run / "run.json").read_text())
    print(json.dumps(herdr(manifest["session"], "agent", "list"), indent=2))


def ensure_dispatcher(run):
    # flock in dispatch is the authority; a resumed client can safely attempt
    # another worker without trusting a stale/reused PID.
    with (run / "delivery.log").open("a") as log:
        subprocess.Popen(["/usr/bin/python3", str(Path(__file__).with_name("main.py")), "_dispatch", str(run)], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)


def dispatch(run):
    import fcntl
    lock = (run / "delivery.lock").open("w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        return
    manifest = json.loads((run / "run.json").read_text())
    misses = 0
    while misses < 30:
        try:
            deliver_pending(run, manifest)
            misses = 0
        except (RuntimeError, OSError, subprocess.TimeoutExpired) as error:
            print(str(error), flush=True)
            misses += 1
        time.sleep(2)


def deliver_pending(run, manifest):
    # Discover every role by its recorded pane. Never target the user's focus
    # or send text into a shell after its agent has exited. Also detects a
    # stopped server when there is no pending mail, so the worker can retire.
    name = manifest["session"]
    live = herdr(name, "agent", "list")
    agents = {item["pane_id"]: item for item in live["agents"]}
    with database(run) as connection:
        pending = connection.execute("SELECT id,sender,recipient,body FROM mail WHERE delivered=0 ORDER BY id").fetchall()
    submitted = set()
    for identifier, sender, recipient, body in pending:
        if recipient in submitted:
            continue
        pane = manifest["panes"][recipient]
        record = agents.get(pane, {})
        state = record.get("state", record.get("agent_status"))
        if state not in ("idle", "done"):
            continue
        herdr(name, "agent", "prompt", pane, f"[ANGI message {identifier} from {sender}]\n{body}")
        with database(run) as connection:
            connection.execute("UPDATE mail SET delivered=1 WHERE id=?", (identifier,))
        submitted.add(recipient)
