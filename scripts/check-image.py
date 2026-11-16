#!/usr/bin/env python3
"""Validate Pi 5 image artifacts without mounting or accessing block devices."""
import hashlib
from pathlib import Path
import struct
import subprocess
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest_region(stream, length):
    digest = hashlib.sha256()
    while length:
        chunk = stream.read(min(length, 1024 * 1024))
        require(chunk, "Truncated partition image")
        digest.update(chunk)
        length -= len(chunk)
    return digest.digest()


def check(images):
    for name in ("Image", "bcm2712-rpi-5-b.dtb", "bcm2712d0-rpi-5-b.dtb",
                 "boot.vfat", "rootfs.ext4", "sdcard.img"):
        require((images / name).is_file(), f"Missing artifact: {name}")
    with (images / "Image").open("rb") as kernel:
        kernel.seek(56)
        require(kernel.read(4) == b"ARM\x64", "Invalid ARM64 kernel header")
    for name in ("bcm2712-rpi-5-b.dtb", "bcm2712d0-rpi-5-b.dtb"):
        with (images / name).open("rb") as dtb:
            require(dtb.read(4) == b"\xd0\x0d\xfe\xed", f"Invalid DTB: {name}")
    with (images / "rootfs.ext4").open("rb") as rootfs:
        rootfs.seek(1080)
        require(rootfs.read(2) == b"\x53\xef", "Invalid ext filesystem magic")
    with (images / "sdcard.img").open("rb") as image:
        mbr = image.read(512)
        require(mbr[510:512] == b"\x55\xaa", "Invalid MBR signature")
        previous_end = 512
        for index, (name, kind) in enumerate((("boot.vfat", 0x0C), ("rootfs.ext4", 0x83))):
            entry = mbr[446 + 16 * index:462 + 16 * index]
            require(entry[4] == kind, f"Wrong partition type: {name}")
            if index == 0:
                require(entry[0] == 0x80, "Boot partition is not bootable")
            start, sectors = struct.unpack_from("<II", entry, 8)
            size = (images / name).stat().st_size
            require(start * 512 >= previous_end, "Overlapping partitions")
            require(size <= sectors * 512, f"Partition too small: {name}")
            require((start + sectors) * 512 <= (images / "sdcard.img").stat().st_size,
                    f"Partition outside disk image: {name}")
            previous_end = (start + sectors) * 512
            image.seek(start * 512)
            with (images / name).open("rb") as source:
                require(digest_region(image, size) == digest_region(source, size),
                        f"Partition contents differ: {name}")
    mtype = images.parent / "host/bin/mtype"
    for name in ("Image", "bcm2712-rpi-5-b.dtb", "bcm2712d0-rpi-5-b.dtb",
                 "config.txt", "cmdline.txt"):
        subprocess.run([str(mtype), "-i", str(images / "boot.vfat"), f"::{name}"],
                       stdout=subprocess.DEVNULL, check=True)
    config = (images / "rpi-firmware/config.txt").read_text()
    require("kernel=Image" in config, "Firmware does not select Image")
    cmdline = (images / "rpi-firmware/cmdline.txt").read_text()
    for token in ("root=/dev/mmcblk0p2", "rootwait", "console=tty1", "console=ttyAMA10,115200"):
        require(token in cmdline.split(), f"Missing boot argument: {token}")
    inittab = (images.parent / "target/etc/inittab").read_text()
    for tty in ("tty1::respawn:", "ttyAMA10::respawn:"):
        require(tty in inittab, f"Missing login console: {tty}")
    target = images.parent / "target"
    require((target / "usr/sbin/dropbear").is_file(), "Missing SSH server")
    dropbear = (target / "etc/default/dropbear").read_text()
    require('-s -p 10.77.0.2:22' in dropbear, "SSH must use key auth on cable address")
    interfaces = (target / "etc/network/interfaces").read_text()
    require("address 10.77.0.2" in interfaces, "Missing Ethernet address")
    keys = target / "root/.ssh/authorized_keys"
    require(keys.is_file() and keys.stat().st_size > 0, "Missing authorised SSH public key")
    require(keys.stat().st_mode & 0o077 == 0, "SSH public key file permissions too open")
    print("PASS: ARM64 kernel, Pi 5 DTBs, boot files, consoles, SSH setup and disk partitions")
    print("Hardware boot remains unverified until tested on a Raspberry Pi 5.")


if __name__ == "__main__":
    try:
        check(Path(sys.argv[1] if len(sys.argv) > 1 else "output/images").resolve())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"FAIL: {error}")
