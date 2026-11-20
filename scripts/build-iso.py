#!/usr/bin/env python3
"""Publishable Pi 4 / 400 ISO from a generic Buildroot output directory."""

import argparse
from pathlib import Path
import subprocess
import sys

from iso_media import BOARD, build_iso, sha256_file


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("board", choices=(BOARD,))
    parser.add_argument("images", type=Path)
    args = parser.parse_args()
    images = args.images.resolve()
    if not (images.parent / ".generic-iso").is_file():
        raise ValueError("Build with AGX_GENERIC_ISO=1 before making a public ISO")
    if (images / "board.txt").read_text().strip() != args.board:
        raise ValueError("Image board does not match the requested ISO")

    checker = Path(__file__).with_name("check-image.py")
    subprocess.run([sys.executable, str(checker), str(images)], check=True)
    iso = images / "angi-rpi4.iso"
    local_xorriso = images.parent / "host/bin/xorriso"
    build_iso(images / "boot.vfat", images / "rootfs.ext4", iso, "generic",
              xorriso=str(local_xorriso) if local_xorriso.is_file() else None)
    (images / (iso.name + ".sha256")).write_text(f"{sha256_file(iso)}  {iso.name}\n")
    print(f"Generic hybrid ISO ready at {iso}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"Could not build the generic ISO: {error}")
