#!/usr/bin/env python3
"""Save ANGI GUI hang evidence on the workstation, across Pi reboots."""

import argparse
import datetime
import logging
from logging.handlers import RotatingFileHandler
import os
from pathlib import Path
import shlex
import signal
import subprocess
import threading


HEALTH = r"""
while true; do
    echo SAMPLE
    cat /proc/sys/kernel/random/boot_id /proc/uptime /proc/loadavg
    grep -E 'MemAvailable|MemFree|Slab' /proc/meminfo
    for p in $(pidof agx-gui); do
        echo "GUI pid=$p"
        grep -E 'Name|State|VmRSS|Threads' /proc/$p/status 2>/dev/null
    done
    for f in /sys/class/thermal/thermal_zone*/temp /sys/class/net/eth0/carrier; do
        echo "$f $(cat "$f" 2>/dev/null)"
    done
    sleep 1
done
"""

STACKS = r"""
while true; do
    echo STACK_SAMPLE
    cat /proc/uptime
    for p in $(pidof agx-gui); do
        for t in /proc/$p/task/*; do
            echo "THREAD $t $(cat "$t/comm" 2>/dev/null)"
            cat "$t/wchan" "$t/stack" 2>/dev/null
            echo
        done
    done
    sleep 0.5
done
"""

LIFECYCLE = r"""
echo BASELINE
uname -a
cat /proc/cmdline /proc/sys/kernel/core_pattern
for p in $(pidof agx-gui); do grep -i core /proc/$p/limits; done
for f in /sys/class/watchdog/watchdog*/bootstatus; do echo "$f $(cat "$f")"; done
while true; do
    echo LIFECYCLE_SAMPLE
    cat /proc/sys/kernel/random/boot_id /proc/uptime
    agx logs --json
    sleep 5
done
"""


def positive_seconds(value):
    seconds = float(value)
    if not 0 < seconds <= 86400:
        raise argparse.ArgumentTypeError("duration must be between 0 and 86400 seconds")
    return seconds


class Capture:
    """Own local readers and their SSH processes, with bounded log storage."""

    def __init__(self, directory):
        self.directory = directory
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.children = set()
        self.threads = []
        self.loggers = []

    def logger(self, name):
        log = logging.getLogger(f"trace.{name}")
        log.setLevel(logging.INFO)
        log.propagate = False
        handler = RotatingFileHandler(
            self.directory / f"{name}.log", maxBytes=8 * 1024 * 1024,
            backupCount=3, encoding="utf-8",
        )
        handler.setFormatter(logging.Formatter("%(asctime)s %(message)s"))
        # Every line is stamped on the workstation; the Pi's clock may reset at boot.
        log.addHandler(handler)
        self.loggers.append(log)
        return log

    def run(self, name, command):
        log = self.logger(name)
        while not self.stop.is_set():
            try:
                with self.lock:
                    if self.stop.is_set():
                        return
                    child = subprocess.Popen(
                        command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                        text=True, errors="replace", start_new_session=True,
                        stdin=subprocess.DEVNULL,
                    )
                    self.children.add(child)
            except OSError as error:
                log.error("START FAILED: %s", error)
                self.stop.wait(3)
                continue
            log.info("CONNECTED reader_pid=%s", child.pid)
            try:
                for line in child.stdout:
                    log.info("%s", line.rstrip("\n"))
            finally:
                child.stdout.close()
                code = child.wait()
                with self.lock:
                    self.children.discard(child)
                log.info("DISCONNECTED exit=%s; retry in 3 seconds", code)
            self.stop.wait(3)

    def start(self, jobs):
        for name, command in jobs.items():
            thread = threading.Thread(target=self.run, args=(name, command), daemon=True)
            self.threads.append(thread)
            thread.start()

    def finish(self):
        self.stop.set()
        with self.lock:
            children = list(self.children)
        for child in children:
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        for child in children:
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        for thread in self.threads:
            thread.join(timeout=4)
        for log in self.loggers:
            for handler in list(log.handlers):
                handler.close()
                log.removeHandler(handler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=os.getenv("AGX_SSH_HOST", "root@10.78.0.2"))
    parser.add_argument("--key", type=Path, default=Path(
        os.getenv("AGX_SSH_KEY", str(Path.home() / ".ssh/id_ed25519"))))
    parser.add_argument("--host-key-alias", default="10.77.0.2")
    parser.add_argument("--duration", type=positive_seconds, default=7200,
                        help="capture lifetime in seconds (default: 7200)")
    parser.add_argument("--output", type=Path, help="new private directory for logs")
    parser.add_argument("--remmina-output", type=Path,
                        help="also follow an existing Remmina stdout/debug log")
    args = parser.parse_args()
    os.umask(0o077)
    directory = args.output or (Path(__file__).resolve().parents[1] / ".cache/gui-traces"
                               / datetime.datetime.now().strftime("%Y%m%d-%H%M%S-%f"))
    directory = directory.resolve()
    directory.mkdir(parents=True, mode=0o700, exist_ok=False)
    ssh = ["ssh", "-i", str(args.key.expanduser()), "-o", "BatchMode=yes",
           "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=3",
           "-o", "ServerAliveCountMax=2", "-o", "StrictHostKeyChecking=yes",
           "-o", f"HostKeyAlias={args.host_key_alias}", "--", args.host]
    # Streams are separate so a blocked stack read cannot stop health or kernel logs.
    jobs = {
        "pi-health": ssh + [HEALTH],
        "pi-stacks": ssh + [STACKS],
        "pi-kernel": ssh + ["cat /dev/kmsg"],
        "pi-gui": ssh + ["tail -n 100 -F /run/agx/gui.log"],
        "pi-syslog": ssh + ["tail -n 100 -F /tmp/messages"],
        "pi-lifecycle": ssh + [LIFECYCLE],
        "workstation-kernel": ["journalctl", "-k", "-f", "-n", "50", "--no-pager"],
        "tunnels": ["journalctl", "--user", "-f", "-n", "20", "--no-pager",
                    "-u", "angi-vnc-tunnel@ethernet", "-u", "angi-vnc-tunnel@wifi"],
        "remmina": ["tail", "-n", "100", "-F", "/tmp/remmina_log_file.log"],
    }
    if args.remmina_output:
        jobs["remmina-debug"] = ["tail", "-n", "100", "-F", str(args.remmina_output.resolve())]
    with (directory / "capture.txt").open("w") as manifest:
        manifest.write(f"Started: {datetime.datetime.now().astimezone().isoformat()}\n"
                       f"Host: {args.host}\nDuration: {args.duration}s\n"
                       "Read-only collection; no binaries or Pi settings changed.\n"
                       "Logs rotate at 8 MiB with three previous files per stream.\n"
                       "No packet, framebuffer, or process-memory capture is performed.\n")
        for name, command in jobs.items():
            manifest.write(f"{name}: {shlex.join(command)}\n")
    print(f"ANGI trace directory: {directory}", flush=True)
    capture = Capture(directory)
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda _sig, _frame: capture.stop.set())
    try:
        capture.start(jobs)
        capture.stop.wait(args.duration)
    finally:
        capture.finish()
        with (directory / "capture.txt").open("a") as manifest:
            manifest.write(f"Stopped: {datetime.datetime.now().astimezone().isoformat()}\n")
        print("ANGI trace stopped; logs retained.", flush=True)


if __name__ == "__main__":
    main()
