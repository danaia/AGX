#!/usr/bin/env python3
"""On the development Pi, test two Wayland clients on one direct Weston DRM session."""
from contextlib import ExitStack
import argparse
import grp
import json
import os
from pathlib import Path
import pwd
import shlex
import signal
import socket
import stat
import subprocess
import tempfile
import time
import urllib.request

GUI = Path("/usr/bin/agx-gui")
SEAT = Path("/run/seatd.sock")
UDEV = Path("/run/udev")
DISPLAY = "wayland-drm-check"
PRIVATE_UDEV = "/usr/libexec/agx-wayland/lib"


def await_ready(predicate, description, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for {description}.")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def start(cleanup, arguments, **options):
    process = subprocess.Popen(arguments, stdin=subprocess.DEVNULL, **options)
    cleanup.callback(stop, process)
    return process


def change_link(target):
    temporary = GUI.with_name(f".agx-gui-drm-check-{os.getpid()}")
    try:
        temporary.symlink_to(target)
        temporary.replace(GUI)
    finally:
        temporary.unlink(missing_ok=True)


def executable(process):
    try:
        return (process / "exe").readlink()
    except OSError:
        return None


def read_evidence(path, uid, limit):
    """Read only ordinary files from the account-owned session directory."""
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid not in (uid, 0) or info.st_size > limit:
            raise RuntimeError(f"Invalid test evidence file: {path.name}")
        return stream.read(limit)


def write_handoff(handoff, runtime, gui, previous, native, mirror):
    variables = "".join(f"{key}={shlex.quote(str(value))}\n"
                        for key, value in dict(runtime=runtime, gui=gui, previous=previous,
                                               stop=handoff.with_name("stop")).items())
    handoff.write_text("#!/bin/sh\n" + variables + f"""
printf '%s\n' "$$" >"$runtime/ready"
attempt=0
while [ ! -S "$runtime/{DISPLAY}" ] && [ ! -e "$stop" ] && [ "$attempt" -lt 150 ]; do
    sleep 0.1
    attempt=$((attempt + 1))
done
if [ -S "$runtime/{DISPLAY}" ] && [ ! -e "$stop" ]; then
    {'export AGX_NATIVE_WAYLAND=1 SLINT_FULLSCREEN=1' if native else ':'}
    XDG_SESSION_TYPE=wayland XDG_RUNTIME_DIR="$runtime" WAYLAND_DISPLAY={DISPLAY} \
        LD_LIBRARY_PATH=/usr/libexec/agx-wayland/egl SLINT_BACKEND=winit-femtovg \
        {'AGX_GUI_VNC=off' if mirror else ''} \
        "$gui" >"$runtime/angi.log" 2>&1 &
    child=$!
    echo "$child" >"$runtime/angi.pid"
    wait "$child"
    unset AGX_NATIVE_WAYLAND SLINT_FULLSCREEN
    attempt=0
    while [ ! -e "$stop" ] && [ "$attempt" -lt 160 ]; do
        sleep 0.1
        attempt=$((attempt + 1))
    done
fi
if [ "$(readlink -f /usr/bin/agx-gui)" = "$previous" ]; then
    exec /usr/bin/agx-gui "$@"
fi
exec "$previous" "$@"
""")
    handoff.chmod(0o755)


def windows(port_file, uid):
    if not port_file.is_file():
        return None
    port = int(read_evidence(port_file, uid, 4096).splitlines()[0])
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/json/list", timeout=2) as response:
            pages = json.load(response)
    except OSError:
        return None
    return next((entry for entry in pages if entry.get("title") == "Example Domain"), None)


def prepare_devices(cleanup):
    # Weston/libinput use real eudev. Seed its temporary database by asking
    # eudev to classify the current devices. Keep ANGI's existing permissions,
    # device-event relay and libudev-zero in place.
    directory = Path(cleanup.enter_context(tempfile.TemporaryDirectory(prefix="angi-udev-", dir="/run")))
    directory.chmod(0o755)
    data = directory / "data"
    data.mkdir()
    for device in sorted(Path("/sys/class/input").glob("event*")):
        result = subprocess.run(["/usr/libexec/agx-wayland/udevadm", "test-builtin", "input_id",
                                 str(device.resolve())], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                text=True, check=True, env=dict(os.environ, LD_LIBRARY_PATH=PRIVATE_UDEV))
        properties = [line for line in result.stdout.splitlines() if line.startswith("ID_INPUT")]
        if properties:
            number = (device / "dev").read_text().strip()
            (data / f"c{number}").write_text("I:1\n" + "".join(f"E:{line}\n" for line in properties))
    UDEV.symlink_to(directory)
    cleanup.callback(UDEV.unlink)


def connected_hdmi():
    outputs = [path.parent.name.removeprefix("card0-")
               for path in Path("/sys/class/drm").glob("card0-HDMI-A-*/status")
               if path.read_text().strip() == "connected"]
    if len(outputs) != 1:
        raise RuntimeError("The native HDMI mirror needs exactly one connected HDMI output.")
    return outputs[0]


def vnc_ready():
    try:
        with socket.create_connection(("127.0.0.1", 5900), timeout=1) as client:
            return client.recv(12).startswith(b"RFB ")
    except OSError:
        return False


def smoke(gui, output, native, hold, mirror, live, google_chrome):
    if os.geteuid() != 0 or not GUI.is_symlink() or SEAT.exists():
        raise RuntimeError("This test needs root on the development Pi and an unused seatd socket.")
    previous = GUI.resolve(strict=True)
    running_desktops = [entry for entry in Path("/proc").iterdir() if entry.name.isdecimal()
                        and executable(entry) == previous]
    if len(running_desktops) != 1:
        raise RuntimeError("Expected one running desktop using the installed ANGI release.")
    pid = int(running_desktops[0].name)
    running = Path(f"/proc/{pid}")
    if executable(running) != previous or b"angi-refresh-on-sigusr1" not in previous.read_bytes():
        raise RuntimeError("Refresh the signed-in desktop to the installed release before this test.")
    account = pwd.getpwuid(Path(f"/proc/{pid}").stat().st_uid)
    if account.pw_uid == 0:
        raise RuntimeError("Sign in to the ANGI desktop before this test.")
    output.mkdir(mode=0o700, exist_ok=True)
    handoff = output / "handoff"
    done = output / "stop"
    done.unlink(missing_ok=True)
    # Its parent must be traversable by the desktop user when the refresh execs it.
    output.chmod(0o711)
    hdmi = connected_hdmi() if mirror else None

    with ExitStack() as cleanup:
        runtime = Path(cleanup.enter_context(tempfile.TemporaryDirectory(prefix="angi-drm-")))
        os.chown(runtime, account.pw_uid, account.pw_gid)
        write_handoff(handoff, runtime, gui, previous, native, mirror)
        env = dict(PATH="/usr/sbin:/usr/bin:/sbin:/bin", HOME=account.pw_dir, LANG="C.UTF-8",
                   USER=account.pw_name, XDG_RUNTIME_DIR=str(runtime), XDG_SESSION_TYPE="wayland",
                   WAYLAND_DISPLAY=DISPLAY, XDG_PICTURES_DIR=str(runtime))
        identity = dict(user=account.pw_uid, group=account.pw_gid,
                        extra_groups=os.getgrouplist(account.pw_name, account.pw_gid), cwd=runtime)

        def retain_evidence():
            for pattern in ("*.log", "wayland-screenshot-*.png"):
                for source in runtime.glob(pattern):
                    (output / source.name).write_bytes(read_evidence(source, account.pw_uid, 32 * 1024 * 1024))

        cleanup.callback(retain_evidence)
        prepare_devices(cleanup)
        with (output / "seatd.log").open("wb") as seat_log:
            seat = start(cleanup, ["/usr/bin/seatd", "-g", grp.getgrgid(account.pw_gid).gr_name],
                         env=dict(env, SEATD_VTBOUND="0"), stdout=seat_log, stderr=seat_log)
            await_ready(lambda: seat.poll() is None and SEAT.is_socket(), "seatd", 5)

        def restore():
            if GUI.is_symlink() and GUI.resolve() == handoff:
                change_link(previous)
            done.touch()
            await_ready(lambda: executable(running) == previous, "return to the normal ANGI desktop")

        cleanup.callback(restore)
        change_link(handoff)
        os.kill(pid, signal.SIGUSR1)
        await_ready((runtime / "ready").is_file, "authenticated desktop handoff", 12)
        change_link(previous)

        weston_env = dict(env, LIBSEAT_BACKEND="seatd", LD_LIBRARY_PATH=PRIVATE_UDEV)
        shell = ["--shell=angi-shell.so"] if native else []
        backend = ["--backend=drm"]
        if mirror or google_chrome:
            config = output / "weston.ini"
            config_text = f"[output]\nname=vnc\nmirror-of={hdmi}\n" if mirror else ""
            if google_chrome:
                config_text += (
                    "\n[launcher]\n"
                    "icon=/var/lib/agx-data/agx-google-chrome/current/product_logo_48.png\n"
                    "path=/usr/bin/google-chrome\n"
                )
            config.write_text(config_text)
            # This root-owned file is opened by Weston after dropping privileges.
            config.chmod(0o644)
            backend.append(f"--config={config}")
        else:
            backend.append("--no-config")
        if mirror:
            backend[0] = "--backend=drm,vnc"
            backend.extend(["--address=127.0.0.1", "--port=5900", "--disable-transport-layer-security"])

        with (runtime / "weston.log").open("wb") as weston_log:
            compositor = start(cleanup, ["/usr/bin/weston", *backend, "--drm-device=card0",
                                         "--renderer=gl", f"--socket={DISPLAY}", "--idle-time=0",
                                         *([] if live else ["--debug"]), *shell],
                               **identity, env=weston_env, stdout=weston_log, stderr=weston_log)
            await_ready(lambda: compositor.poll() is None and (runtime / DISPLAY).is_socket(),
                        "Weston DRM socket", 10)

        if live:
            angi_pid = runtime / "angi.pid"
            await_ready(angi_pid.is_file, "the ANGI shell")
            child = Path("/proc") / read_evidence(angi_pid, account.pw_uid, 32).decode().strip()
            await_ready(lambda: executable(child) == gui, "the ANGI Wayland process")
            if google_chrome:
                with (runtime / "google-chrome.log").open("wb") as chrome_log:
                    start(cleanup, ["/usr/bin/google-chrome", "--no-first-run",
                                    "--no-default-browser-check", "--new-window", "https://example.com"],
                          **identity, env=env, stdout=chrome_log, stderr=chrome_log)
            if mirror:
                try:
                    await_ready(vnc_ready, "Weston's loopback VNC mirror", 10)
                except RuntimeError as error:
                    print(f"{error} The HDMI desktop remains active.", flush=True)
            session_pid = output / "session.pid"
            session_pid.write_text(f"{os.getpid()}\n")
            cleanup.callback(session_pid.unlink, missing_ok=True)
            print("ANGI is running as a native Wayland session on HDMI.", flush=True)
            while compositor.poll() is None and executable(child) == gui:
                time.sleep(0.5)
            if compositor.poll() is not None:
                raise RuntimeError(f"Weston exited with status {compositor.returncode}.")
            return

        if mirror:
            await_ready(vnc_ready, "Weston's loopback VNC mirror", 10)

        with (runtime / "chromium.log").open("wb") as chrome_log:
            profile = runtime / "chromium-profile"
            launcher = "/usr/bin/google-chrome" if google_chrome else "/usr/bin/angi-chromium"
            chrome = start(cleanup, [launcher, "--new-window", "https://example.com",
                                     "--no-first-run", f"--user-data-dir={profile}", "--remote-debugging-port=0"],
                           **identity, env=dict(env, WAYLAND_DEBUG="client"), stdout=chrome_log, stderr=chrome_log)
            await_ready(lambda: chrome.poll() is None and windows(profile / "DevToolsActivePort", account.pw_uid),
                        "the browser rendering Example Domain", 20)

        def mapped_clients():
            if not (runtime / "angi.pid").is_file():
                return False
            angi_pid = read_evidence(runtime / "angi.pid", account.pw_uid, 32).decode().strip()
            scene = subprocess.check_output(["/usr/bin/weston-debug", "scene-graph"], **identity,
                                            env=env, timeout=3, text=True)
            (runtime / "scene.log").write_text(scene)
            return bool(angi_pid and any(f"PID {angi_pid}," in line for line in scene.splitlines())
                        and "Example Domain" in scene)

        await_ready(mapped_clients, "both independent clients in Weston's scene", 8)
        # The title arrives before Chromium's renderer paints the page.
        time.sleep(1.5)
        subprocess.run(["/usr/bin/weston-screenshooter"], **identity, env=env, timeout=10, check=True)
        if not list(runtime.glob("wayland-screenshot-*.png")):
            raise RuntimeError("Weston did not save the HDMI framebuffer capture.")
        print("Weston DRM, ANGI and the browser verified. The browser rendered Example Domain.", flush=True)
        if hold:
            print(f"Keeping the test session available for {hold} seconds.", flush=True)
            time.sleep(hold)


def interrupted(_signum, _frame):
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGHUP, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("gui", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--hold", type=int, choices=range(0, 301), metavar="SECONDS", default=0)
    parser.add_argument("--mirror", action="store_true", help="mirror HDMI on the loopback VNC port")
    parser.add_argument("--session", action="store_true", help="keep the native HDMI and VNC session active")
    shell = parser.add_mutually_exclusive_group()
    shell.add_argument("--native", action="store_true", help="load ANGI's native Chromium shell adapter")
    shell.add_argument("--google-chrome", action="store_true",
                       help="use Weston's desktop shell with Google Chrome as a separate window")
    options = parser.parse_args()
    native = options.native or (options.session and not options.google_chrome)
    mirror = options.mirror or (options.session and native)
    smoke(options.gui.resolve(strict=True), options.output.resolve(), native,
          options.hold, mirror, options.session, options.google_chrome)
