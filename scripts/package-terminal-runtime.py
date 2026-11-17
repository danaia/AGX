#!/usr/bin/env python3
"""Package native CLI tools and their dependency closure without boot libraries."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

SYSTEM = {"libc.so.6", "libm.so.6", "libpthread.so.0", "librt.so.1", "libdl.so.2",
          "libgcc_s.so.1", "libatomic.so.1", "libresolv.so.2", "libutil.so.1", "ld-linux-aarch64.so.1"}
TOOLS = ("bin/bash", "usr/bin/tmux", "usr/bin/less", "usr/bin/vim", "usr/bin/ssh",
         "usr/bin/scp", "usr/bin/sftp", "usr/bin/ssh-agent", "usr/bin/ssh-add",
         "usr/bin/ssh-keygen", "usr/bin/ssh-keyscan", "usr/bin/tput", "usr/bin/reset", "usr/bin/clear")


def package(target, output):
    selected = set()

    def add(name):
        name = os.path.normpath(name).lstrip("/")
        if name == ".." or name.startswith(("../", "lib/", "lib64/")):
            raise ValueError(f"Refusing boot library or outside path: {name}")
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
            for child in sorted(path.iterdir()):
                add(str(child.relative_to(target)))
        elif path.is_file():
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

    for name in TOOLS:
        add(name)
    for name in ("usr/share/terminfo", "usr/share/vim"):
        add(name)
    for path in sorted(target.glob("usr/libexec/ssh*")):
        add(str(path.relative_to(target)))
    for name in ("usr/share/terminfo/x/xterm-256color", "usr/share/terminfo/t/tmux-256color"):
        if name not in selected:
            raise ValueError(f"Missing required terminal description: {name}")
    root = Path(__file__).resolve().parents[1]
    strip = root / "output/host/bin/aarch64-linux-strip"

    def owner(info):
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        info.mtime = 0
        return info

    with tempfile.TemporaryDirectory(prefix="agx-terminal-") as temporary, tarfile.open(output, "w", dereference=False) as archive:
        for index, name in enumerate(sorted(selected)):
            path = target / name
            source = path
            if path.is_file() and not path.is_symlink():
                with path.open("rb") as stream:
                    elf = stream.read(4) == b"\x7fELF"
                if elf:
                    source = Path(temporary) / str(index)
                    subprocess.run([str(strip), "--strip-unneeded", "-o", str(source), str(path)], check=True)
                    source.chmod(path.stat().st_mode & 0o755)
            archive.add(source, arcname=name, recursive=False, filter=owner)
        # Use the external toolchain's own target localedef and locale sources.
        # The appliance generates compatible UTF-8 data on NVMe before activation.
        sysroot = root / "output/host/aarch64-buildroot-linux-gnu/sysroot"
        source_root = sysroot / "usr/share/i18n"
        sources = set()
        def locale_source(name):
            if name in sources:
                return
            sources.add(name)
            path = source_root / "locales" / name
            for child in re.findall(r'^\s*(?:copy|include)\s+"([^"/]+)"', path.read_text(), re.MULTILINE):
                locale_source(child)
        locale_source("C")
        archive.add(sysroot / "usr/bin/localedef", arcname="usr/libexec/agx-terminal-localedef", filter=owner)
        for name in sorted(sources):
            archive.add(source_root / "locales" / name, arcname=f"usr/share/agx-terminal/i18n/locales/{name}", filter=owner)
        archive.add(source_root / "charmaps/UTF-8.gz", arcname="usr/share/agx-terminal/i18n/charmaps/UTF-8.gz", filter=owner)
        archive.add(root / "buildroot/board/agx/rootfs-overlay/usr/share/agx/terminal/bashrc", arcname="usr/share/agx/terminal/bashrc", filter=owner)
        archive.add(root / "buildroot/board/agx/rootfs-overlay/etc/profile.d/agx-welcome.sh", arcname="etc/profile.d/agx-welcome.sh", filter=owner)
        archive.add(root / "buildroot/board/agx/rootfs-overlay/etc/profile.d/agx-terminal.sh",
                    arcname="etc/profile.d/agx-terminal.sh", filter=owner)
    print(f"Packaged {len(selected)} terminal runtime paths: {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        package(args.target, args.output)
    except (ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")
