#!/usr/bin/env python3
"""Fetch verified native ARM64 development tools into a relocatable private SDK."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache/angi-dev"
SDK = CACHE / "sdk"


def fetch(package):
    archive = CACHE / Path(package["Filename"]).name
    if not archive.exists():
        urllib.request.urlretrieve("https://deb.debian.org/debian/" + package["Filename"], archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != package["SHA256"]:
        raise ValueError(f"Checksum mismatch: {archive}")
    return archive


def main():
    CACHE.mkdir(parents=True, exist_ok=True)
    SDK.mkdir(exist_ok=True)
    packages = json.loads((ROOT / "apps/agent/sdk-packages.json").read_text())
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        archives = list(pool.map(fetch, packages))
    for archive in archives:
        members = subprocess.check_output(["ar", "t", str(archive)], text=True).splitlines()
        member = next(name for name in members if name.startswith("data.tar"))
        data = subprocess.check_output(["ar", "p", str(archive), member])
        subprocess.run(["tar", "-xJf", "-", "-C", str(SDK)], input=data, check=True)
    # Never deploy Debian's libc, dynamic loader, or live /lib paths. These
    # packages contain tools and private dependencies only; the Pi supplies libc.
    for directory in (SDK / "lib", SDK / "usr/lib/aarch64-linux-gnu"):
        if directory.exists():
            for path in directory.rglob("*"):
                if path.name.startswith(("libc.so", "ld-linux")):
                    raise ValueError(f"Unexpected boot library: {path}")
    print(f"Native ARM64 development tools prepared at {SDK}")


if __name__ == "__main__":
    main()
