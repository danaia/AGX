#!/usr/bin/env python3
"""Package the pinned OpenCode CLI and Git/search tools; preserve its embedded runtime."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

SYSTEM = {"libc.so.6", "libm.so.6", "libpthread.so.0", "librt.so.1", "libdl.so.2",
          "libgcc_s.so.1", "libatomic.so.1", "libresolv.so.2", "libutil.so.1", "ld-linux-aarch64.so.1"}


def package(target, output):
    selected = set()
    def add(name):
        name = os.path.normpath(name).lstrip("/")
        if name == ".." or name.startswith(("../", "lib/", "lib64/")):
            raise ValueError(f"Refusing boot or outside path: {name}")
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
        else:
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
    for name in ("usr/bin/git", "usr/bin/rg", "usr/libexec/git-core", "usr/share/git-core"):
        add(name)
    root = Path(__file__).resolve().parents[1]
    binary = target / "usr/libexec/opencode/opencode"
    if not binary.is_file():
        raise ValueError("Run scripts/build-opencode.sh first")
    def owner(info):
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        info.mtime = 0
        return info
    with tempfile.TemporaryDirectory(prefix="agx-opencode-") as temporary, tarfile.open(output, "w", dereference=False) as archive:
        inodes = {}
        for index, name in enumerate(sorted(selected)):
            path = target / name
            source = path
            if path.is_file() and not path.is_symlink():
                metadata = path.stat()
                inode = (metadata.st_dev, metadata.st_ino)
                if inode in inodes:
                    info = archive.gettarinfo(str(path), arcname=name)
                    info.type = tarfile.LNKTYPE
                    info.linkname = inodes[inode]
                    info.size = 0
                    archive.addfile(owner(info))
                    continue
                inodes[inode] = name
            if path.is_file() and not path.is_symlink():
                with path.open("rb") as stream:
                    elf = stream.read(4) == b"\x7fELF"
                if elf:
                    source = Path(temporary) / str(index)
                    subprocess.run([str(root / "output/host/bin/aarch64-linux-strip"), "--strip-unneeded", "-o", str(source), str(path)], check=True)
                    source.chmod(path.stat().st_mode & 0o755)
            archive.add(source, arcname=name, recursive=False, filter=owner)
        # Bun's executable contains an embedded application; stripping can break it.
        archive.add(binary, arcname="usr/libexec/opencode/opencode", filter=owner)
        archive.add(root / "apps/opencode/launch.sh", arcname="usr/bin/opencode", filter=owner)
        archive.add(root / "apps/opencode/LICENSE.upstream", arcname="usr/share/licenses/opencode/LICENSE", filter=owner)
        archive.add(root / "buildroot/board/agx/rootfs-overlay/etc/init.d/S12agx-data", arcname="etc/init.d/S12agx-data", filter=owner)
    print(f"Packaged OpenCode and {len(selected)} Git/search runtime paths: {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    package(args.target, args.output)
