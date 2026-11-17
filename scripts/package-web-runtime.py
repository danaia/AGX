#!/usr/bin/env python3
"""Package ANGI Web and its ELF dependency closure without replacing boot libraries."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

SYSTEM = {"libc.so.6", "libm.so.6", "libpthread.so.0", "librt.so.1", "libdl.so.2",
          "libgcc_s.so.1", "libatomic.so.1", "libresolv.so.2", "libutil.so.1", "ld-linux-aarch64.so.1"}


def package(target, helper, output):
    selected = set()

    def add(name):
        name = os.path.normpath(name).lstrip("/")
        if name == ".." or name.startswith("../"):
            raise ValueError(f"Path outside target: {name}")
        if name.startswith(("lib/", "lib64/")):
            raise ValueError(f"Refusing to package boot dependency: {name}")
        if name in selected:
            return
        path = target / name
        if not path.exists() and not path.is_symlink():
            raise ValueError(f"Missing runtime file: {name}")
        selected.add(name)
        if path.is_symlink():
            link = os.readlink(path)
            add(link.lstrip("/") if link.startswith("/") else str(Path(name).parent / link))
        elif path.is_dir():
            for child in path.iterdir():
                add(str(child.relative_to(target)))
        elif path.is_file():
            dependencies(path)

    def dependencies(path):
        with path.open("rb") as stream:
            if stream.read(4) != b"\x7fELF":
                return
        dynamic = subprocess.check_output(["readelf", "-d", str(path)], text=True)
        for library in re.findall(r"\(NEEDED\).*\[([^]]+)\]", dynamic):
            if library in SYSTEM:
                continue
            dependency = next((f"{directory}/{library}" for directory in ("usr/lib", "usr/lib64", "lib", "lib64")
                               if (target / directory / library).exists()), None)
            if dependency is None:
                raise ValueError(f"{path} needs missing {library}")
            add(dependency)

    dependencies(helper)
    for library in ("libWPEWebKit-2.0.so.*",):
        matches = list(target.glob(f"usr/lib/{library}"))
        if not matches:
            raise ValueError(f"Build WPE first: missing {library}")
        for path in matches:
            add(str(path.relative_to(target)))
    # Loadable TLS provider, subprocesses, injected bundle/resources, fonts and HTTPS roots.
    for pattern in ("usr/libexec/wpe-webkit-2.0", "usr/lib/wpe-webkit-2.0", "usr/share/wpe-webkit-2.0",
                    "usr/lib/gio/modules", "usr/lib/wpe-platform-2.0", "usr/lib/pkcs11", "usr/share/p11-kit", "etc/pkcs11", "usr/share/mime", "usr/share/fonts/dejavu", "usr/share/fonts/truetype/dejavu"):
        if (target / pattern).exists():
            add(pattern)
    for required in ("usr/bin/bwrap", "usr/bin/xdg-dbus-proxy", "etc/ssl/certs/ca-certificates.crt"):
        add(required)
    if not any("WebProcess" in path for path in selected):
        raise ValueError("Missing WPE WebProcess runtime")
    if not any("libgiognutls" in path for path in selected):
        raise ValueError("Missing HTTPS GIO provider")

    def owner(info):
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        info.mtime = 0
        return info

    root = Path(__file__).resolve().parents[1]
    strip = root / "output/host/bin/aarch64-linux-strip"
    if not strip.is_file():
        raise ValueError("Missing cross strip tool")
    with tempfile.TemporaryDirectory(prefix="agx-web-package-") as temporary, tarfile.open(output, "w", dereference=False) as archive:
        for index, name in enumerate(sorted(selected)):
            path = target / name
            source = path
            if path.is_file() and not path.is_symlink():
                with path.open("rb") as stream:
                    elf = stream.read(4) == b"\x7fELF"
                if elf:
                    source = Path(temporary) / str(index)
                    subprocess.run([str(strip), "--strip-unneeded", "-o", str(source), str(path)], check=True)
                    source.chmod(path.stat().st_mode & 0o777)
            archive.add(source, arcname=name, recursive=False, filter=owner)
        archive.add(helper, arcname="usr/libexec/agx-web", filter=owner)
        archive.add(root / "apps/web/start.html", arcname="usr/share/agx/web/start.html", filter=owner)
    print(f"Packaged {len(selected)} runtime paths: {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", type=Path)
    parser.add_argument("helper", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        package(args.target.resolve(), args.helper.resolve(), args.output)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"FAIL: {error}\n")
