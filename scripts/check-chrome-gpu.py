#!/usr/bin/env python3
"""Inspect Chrome's GPU backend from a temporary profile in an active Wayland session."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import tempfile
import time
from urllib.parse import urlsplit


class DevTools:
    """Minimal local WebSocket transport for the fixed CDP queries below."""

    def __init__(self, address):
        url = urlsplit(address)
        if url.hostname != "127.0.0.1":
            raise ValueError("Expected a loopback DevTools address")
        self.socket = socket.create_connection((url.hostname, url.port), timeout=12)
        self.input = self.socket.makefile("rb")
        key = base64.b64encode(os.urandom(16))
        self.socket.sendall(
            f"GET {url.path} HTTP/1.1\r\nHost: {url.netloc}\r\n".encode()
            + b"Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\n"
            + b"Sec-WebSocket-Key: " + key + b"\r\n\r\n"
        )
        status = self.input.readline()
        headers = {}
        while (line := self.input.readline()) not in (b"\r\n", b""):
            name, _, value = line.partition(b":")
            headers[name.lower()] = value.strip()
        accept = base64.b64encode(hashlib.sha1(key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest())
        if b" 101 " not in status or headers.get(b"sec-websocket-accept") != accept:
            self.close()
            raise RuntimeError("Chrome rejected the DevTools connection")

    def close(self):
        self.input.close()
        self.socket.close()

    def read(self, count):
        data = self.input.read(count)
        if len(data) != count:
            raise EOFError("Chrome closed DevTools")
        return data

    def query(self, method, params=None):
        data = json.dumps({"id": 1, "method": method, "params": params or {}}).encode()
        mask = os.urandom(4)
        size = bytes([len(data) | 128]) if len(data) < 126 else b"\xfe" + struct.pack("!H", len(data))
        self.socket.sendall(b"\x81" + size + mask + bytes(value ^ mask[i % 4] for i, value in enumerate(data)))

        fragments = bytearray()
        while True:
            head, size = self.read(2)
            length = size & 127
            if length == 126:
                length = int.from_bytes(self.read(2), "big")
            if length == 127:
                length = int.from_bytes(self.read(8), "big")
            if len(fragments) + length > 4 * 1024 * 1024:
                raise ValueError("Unexpectedly large DevTools response")
            fragments.extend(self.read(length))
            if head & 0x80:
                response = json.loads(fragments)
                fragments.clear()
                if response.get("id") == 1:
                    if "error" in response:
                        raise RuntimeError(response["error"])
                    return response["result"]


def check(executable, flags):
    with tempfile.TemporaryDirectory(prefix="angi-chrome-gpu-") as name:
        directory = Path(name)
        profile = directory / "profile"
        command = [executable, *flags, f"--user-data-dir={profile}", "--remote-debugging-port=0",
                   "--no-first-run", "--disable-background-networking", "about:blank"]
        with (directory / "chrome.log").open("w+") as log:
            browser = subprocess.Popen(command, stdout=log, stderr=log, start_new_session=True)
            try:
                port_file = profile / "DevToolsActivePort"
                deadline = time.monotonic() + 20
                while not port_file.is_file() and browser.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.1)
                if not port_file.is_file():
                    raise RuntimeError("Chrome did not start DevTools")
                port, path = port_file.read_text().splitlines()[:2]
                client = DevTools(f"ws://127.0.0.1:{port}{path}")
                try:
                    version = client.query("Browser.getVersion")["product"]
                    gpu = client.query("SystemInfo.getInfo")["gpu"]
                finally:
                    client.close()
                attrs = gpu.get("auxAttributes", {})
                return {
                    "flag_overrides": flags,
                    "chrome": version,
                    "devices": gpu["devices"],
                    "renderer": attrs.get("glRenderer"),
                    "display_type": attrs.get("displayType"),
                    "implementation": attrs.get("glImplementationParts"),
                    "features": gpu.get("featureStatus"),
                }
            except (OSError, ValueError, RuntimeError, EOFError):
                log.seek(0)
                print(log.read()[-7000:])
                raise
            finally:
                if browser.poll() is None:
                    os.killpg(browser.pid, signal.SIGTERM)
                    try:
                        browser.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        os.killpg(browser.pid, signal.SIGKILL)
                        browser.wait()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("launcher")
    parser.add_argument("flags", nargs=argparse.REMAINDER)
    options = parser.parse_args()
    # A second dash separates flags for Chrome from the diagnostic's options.
    flags = options.flags[1:] if options.flags[:1] == ["--"] else options.flags
    print(json.dumps(check(options.launcher, flags), indent=2))
