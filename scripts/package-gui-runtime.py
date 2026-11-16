#!/usr/bin/env python3
"""Package the Pi GUI's libraries, Mesa drivers and their ELF dependencies."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tarfile


def package(target, output):
    selected = set()
    search = ["usr/lib", "lib", "lib64", "usr/lib64"]
    # Supplied by the existing AGX image/toolchain. Never overwrite live libc
    # or its loader while PID 1, SSH and recovery services are using them.
    system = {"libc.so.6", "libm.so.6", "libpthread.so.0", "librt.so.1", "libdl.so.2", "libgcc_s.so.1",
              "libresolv.so.2", "libutil.so.1", "ld-linux-aarch64.so.1"}

    def add(name):
        name = os.path.normpath(name).lstrip("/")
        if name == ".." or name.startswith("../"):
            raise ValueError(f"Library outside target tree: {name}")
        # Every /lib dependency must remain on the SD root: PID 1 and the
        # programs that mount NVMe must load before the data volume exists.
        if name.startswith(("lib/", "lib64/")):
            return
        if name in selected:
            return
        path = target / name
        if not path.exists() and not path.is_symlink():
            raise ValueError(f"Missing runtime file: {name}")
        selected.add(name)
        if path.is_symlink():
            link = os.readlink(path)
            add(link.lstrip("/") if link.startswith("/") else str(Path(name).parent / link))
        elif path.is_file():
            with path.open("rb") as stream:
                elf = stream.read(4) == b"\x7fELF"
            if elf:
                dynamic = subprocess.check_output(["readelf", "-d", str(path)], text=True)
                for library in re.findall(r"\(NEEDED\).*\[([^]]+)\]", dynamic):
                    if library in system:
                        continue
                    dependency = next((f"{directory}/{library}" for directory in search
                                       if (target / directory / library).exists()), None)
                    if dependency is None:
                        raise ValueError(f"{name} requires missing {library}")
                    add(dependency)

    for library in ("libinput", "libevdev", "libudev", "libxkbcommon", "libfontconfig",
                    "libEGL", "libGLESv2", "libgbm"):
        files = list(target.glob(f"usr/lib/{library}.so.*"))
        if not files:
            raise ValueError(f"Build desktop and Mesa packages first: missing {library}")
        for path in files:
            add(str(path.relative_to(target)))
    # Mesa 26 embeds the selected drivers in libgallium; older Mesa releases
    # install per-driver DRI modules. GBM's dynamically loaded backend is separate.
    gallium = list(target.glob("usr/lib/libgallium*.so*"))
    if gallium:
        for path in gallium:
            add(str(path.relative_to(target)))
    else:
        for driver in ("v3d", "vc4"):
            add(f"usr/lib/dri/{driver}_dri.so")
    for path in target.glob("usr/lib/gbm/*.so"):
        add(str(path.relative_to(target)))
    if (target / "usr/share/drirc.d").is_dir():
        for path in (target / "usr/share/drirc.d").rglob("*"):
            add(str(path.relative_to(target)))
    for directory in ("usr/share/libinput", "usr/share/X11/xkb", "usr/share/fontconfig", "etc/fonts"):
        add(directory)
        for path in (target / directory).rglob("*"):
            add(str(path.relative_to(target)))

    def owner(info):
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        return info

    with tarfile.open(output, "w", dereference=False) as archive:
        for name in sorted(selected):
            archive.add(target / name, arcname=name, recursive=False, filter=owner)
    print(f"Packaged {len(selected)} GUI runtime paths; SD boot libraries excluded: {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        package(args.target.resolve(), args.output)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"FAIL: {error}\n")
