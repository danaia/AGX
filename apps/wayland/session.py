"""Start the private Wayland session used by the ANGI Chromium window."""
from contextlib import ExitStack
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import sys
import tempfile
import time


def request(arguments):
    directory = Path(os.environ.get("XDG_RUNTIME_DIR") or Path.home() / ".cache/angi")
    message = ("chromium " + json.dumps(arguments) + "\n").encode()
    if len(message) >= 8192:
        raise RuntimeError("The Chromium command is too long.")
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(5)
        client.connect(str(directory / "angi-open.sock"))
        client.sendall(message)
        with client.makefile("r", encoding="utf-8") as reply:
            result = reply.readline(4096).strip()
        if result != "ok":
            raise RuntimeError(result.removeprefix("error ") or "The ANGI desktop did not answer.")


def start(cleanup, command, environment):
    process = subprocess.Popen(command, env=environment, stdin=subprocess.DEVNULL,
                               stdout=sys.stderr, start_new_session=True)

    def stop():
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()

    cleanup.callback(stop)
    return process


def await_socket(path, process):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline and process.poll() is None:
        if path.is_socket():
            return
        time.sleep(0.05)
    raise RuntimeError(f"{Path(process.args[0]).name} did not start.")


def close_browser(pid_file, cage):
    if cage.poll() is not None:
        return
    try:
        pid = int(pid_file.read_text())
        if pid > 1 and os.getpgid(pid) == cage.pid:
            # Let Chromium flush its profile while the compositor and D-Bus are alive.
            os.kill(pid, signal.SIGTERM)
            cage.wait(timeout=2)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        pass


def run(arguments):
    with ExitStack() as cleanup:
        runtime = Path(cleanup.enter_context(tempfile.TemporaryDirectory(prefix="angi-chromium-")))
        render_node = "/dev/dri/renderD128"
        pid_file = runtime / "chromium.pid"
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), WLR_BACKENDS="headless", WLR_HEADLESS_OUTPUTS="1",
                   AGX_WAYLAND_CHROMIUM_PID=str(pid_file),
                   WLR_RENDERER="gles2" if os.access(render_node, os.R_OK | os.W_OK) else "pixman")
        if env["WLR_RENDERER"] == "gles2":
            env["WLR_RENDER_DRM_DEVICE"] = render_node
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET"):
            env.pop(key, None)
        compat = "/usr/libexec/agx-wayland/lib"
        libraries = env.get("LD_LIBRARY_PATH")
        env["LD_LIBRARY_PATH"] = f"{compat}:{libraries}" if libraries else compat
        cage = start(cleanup, ["/usr/bin/cage", "--", "/usr/bin/angi-chromium", *arguments], env)
        display = runtime / "wayland-0"
        await_socket(display, cage)

        endpoint = runtime / "viewer"
        vnc = start(cleanup, ["/usr/bin/wayvnc", "--config=/dev/null", "--unix-socket", str(endpoint)],
                    dict(env, WAYLAND_DISPLAY=display.name))
        cleanup.callback(close_browser, pid_file, cage)
        await_socket(endpoint, vnc)
        print(endpoint, flush=True)
        desktop = select.poll()
        # A closed read end means the desktop exited or refreshed.
        desktop.register(sys.stdout, select.POLLERR)
        while cage.poll() is None and vnc.poll() is None and not desktop.poll(200):
            pass
        return cage.returncode or vnc.returncode or 0


def interrupted(signum, _frame):
    raise SystemExit(128 + signum)


if __name__ == "__main__":
    signal.signal(signal.SIGHUP, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    try:
        if sys.argv[1] == "--serve":
            sys.exit(run(sys.argv[2:]))
        request(sys.argv[2:])
    except (OSError, RuntimeError) as error:
        sys.exit(f"Chromium: {error}")
