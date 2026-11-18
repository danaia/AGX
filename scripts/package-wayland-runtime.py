#!/usr/bin/env python3
"""Package the Wayland runtime for the NVMe: only files the running image does not already have.

    package-wayland-runtime.py --target output-wayland/target --image output/target --output runtime.tar

Seeds (the compositor host, test clients and Chromium's missing libraries) are expanded to their ELF dependency
closure inside --target. A file is shipped only when --image lacks a file at that path, so nothing that is
already on the device (Mesa, libinput, libxkbcommon, libc ...) can be replaced. Files under /lib and /lib64 stay
on the SD root. The versioned eudev library is packaged under a Chromium-private path.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tarfile

# What ANGI's Wayland host and Chromium need beyond the image. Globs are relative to --target.
SEEDS = (
    "usr/bin/cage", "usr/bin/foot", "usr/bin/wayland-info", "usr/bin/dbus-run-session", "usr/bin/wayvnc",
    "usr/bin/seatd", "usr/bin/weston", "usr/bin/weston-debug", "usr/bin/weston-screenshooter",
    "usr/libexec/weston-desktop-shell", "usr/libexec/weston-keyboard",
    "usr/lib/libweston-*/drm-backend.so", "usr/lib/libweston-*/gl-renderer.so",
    "usr/lib/libweston-*/vnc-backend.so",
    "usr/lib/weston/desktop-shell.so", "usr/lib/weston/angi-shell.so", "usr/lib/libseat.so.*",
    "usr/lib/libwayland-client.so.*", "usr/lib/libwayland-server.so.*", "usr/lib/libwayland-cursor.so.*",
    "usr/lib/libwayland-egl.so.*",
    "usr/lib/libwlroots*.so*",
    # The 18 libraries Chromium's main binary requires and the Pi lacks.
    "usr/lib/libnspr4.so", "usr/lib/libnss3.so", "usr/lib/libnssutil3.so", "usr/lib/libsmime3.so",
    "usr/lib/libatk-1.0.so.*", "usr/lib/libatk-bridge-2.0.so.*", "usr/lib/libatspi.so.*",
    "usr/lib/libcups.so.*", "usr/lib/libxcb.so.*", "usr/lib/libasound.so.*", "usr/lib/libX11.so.*",
    "usr/lib/libXext.so.*", "usr/lib/libXcomposite.so.*", "usr/lib/libXdamage.so.*", "usr/lib/libXfixes.so.*",
    "usr/lib/libXrandr.so.*", "usr/lib/libcairo.so.*", "usr/lib/libpango-1.0.so.*",
    # NSS loads these by name, so they do not appear in Chromium's NEEDED entries.
    "usr/lib/libnssckbi.so", "usr/lib/libsoftokn3.so", "usr/lib/libfreebl3.so", "usr/lib/libnssdbm3.so",
)
VIEWER = Path(__file__).resolve().parent.parent / "apps/wayland"
# Shared data the seeds read at run time.
DATA = ("usr/share/alsa", "usr/share/X11/locale", "usr/share/weston", "etc/fonts")
NEVER = {"libudev.so.1", "libudev.so", "libc.so.6", "libm.so.6", "libpthread.so.0", "librt.so.1", "libdl.so.2",
         "libgcc_s.so.1", "libresolv.so.2", "libutil.so.1", "ld-linux-aarch64.so.1", "libstdc++.so.6"}
SEARCH = ("usr/lib", "lib", "lib64", "usr/lib64", "usr/lib/weston")


def package(target, image, output):
    target, image = Path(target).resolve(), Path(image)
    selected, closure_skipped = {}, set()

    def present_in_image(name):
        return (image / name).exists() or (image / name).is_symlink()

    def add(name, forced=False):
        name = os.path.normpath(name).lstrip("/")
        if name.startswith(("../", "lib/", "lib64/")) or name == "..":
            return                                     # the SD root keeps these; never ship them
        if name in selected or name in closure_skipped:
            return
        path = target / name
        if not path.exists() and not path.is_symlink():
            raise ValueError(f"Missing file in the build tree: {name}")
        if path.name in NEVER or (present_in_image(name) and not forced):
            closure_skipped.add(name)                  # already on the device
            return
        selected[name] = path
        if path.is_symlink():
            link = os.readlink(path)
            add(link.lstrip("/") if link.startswith("/") else str(Path(name).parent / link), forced)
        elif path.is_file() and path.open("rb").read(4) == b"\x7fELF":
            dynamic = subprocess.check_output(["readelf", "-d", str(path)], text=True)
            for library in re.findall(r"\(NEEDED\).*\[([^]]+)\]", dynamic):
                if library in NEVER:
                    continue
                found = next((f"{d}/{library}" for d in SEARCH if (target / d / library).exists()), None)
                if found is None:
                    raise ValueError(f"{name} needs {library}, which is in neither the build tree nor the image")
                add(found)

    missing = []
    for pattern in SEEDS + ("usr/bin/agx-wl",):
        matches = sorted(target.glob(pattern))
        if not matches and pattern in SEEDS:
            missing.append(pattern)
        for path in matches:
            add(str(path.relative_to(target)), forced=True)
    for directory in DATA:
        for path in sorted((target / directory).rglob("*")) if (target / directory).is_dir() else []:
            if path.is_file() or path.is_symlink():
                add(str(path.relative_to(target)))
    if missing:
        raise ValueError("Required Wayland runtime files were not built: " + ", ".join(missing))

    udev = (target / "lib/libudev.so.1").resolve(strict=True)
    if not udev.is_relative_to(target) or not udev.is_file():
        raise ValueError("The versioned eudev library must come from the Wayland build tree")
    selected["usr/libexec/agx-wayland/lib/libudev.so.1"] = udev
    selected["usr/libexec/agx-wayland/lib/libblkid.so.1"] = (target / "lib/libblkid.so.1").resolve(strict=True)
    selected["usr/libexec/agx-wayland/udevadm"] = target / "usr/bin/udevadm"
    # The appliance Mesa build has no Wayland EGL platform. Stage the
    # matching Mesa EGL front end separately for the winit client.
    selected["usr/libexec/agx-wayland/egl/libEGL.so.1"] = (target / "usr/lib/libEGL.so.1").resolve(strict=True)
    selected["usr/bin/angi-chromium"] = VIEWER / "chromium.sh"
    selected["usr/libexec/agx-wayland/session.py"] = VIEWER / "session.py"

    def owner(info):
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        if info.name == "usr/bin/angi-chromium":
            info.mode = 0o755
        return info

    with tarfile.open(output, "w") as archive:
        for name in sorted(selected):
            archive.add(selected[name], arcname=name, recursive=False, filter=owner)
    return sorted(selected), sorted(closure_skipped)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    files, skipped = package(args.target, args.image, args.output)
    size = Path(args.output).stat().st_size
    print(f"{len(files)} files ({size / 1e6:.1f} MB archive) shipped; {len(skipped)} dependencies already on the device")
