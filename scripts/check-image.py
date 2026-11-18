#!/usr/bin/env python3
"""Validate Pi 5 image artifacts without mounting or accessing block devices."""
import hashlib
from pathlib import Path
import re
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
                 "config.txt", "cmdline.txt", "overlays/dwc2.dtbo"):
        subprocess.run([str(mtype), "-i", str(images / "boot.vfat"), f"::{name}"],
                       stdout=subprocess.DEVNULL, check=True)
    for name in ("config.txt", "cmdline.txt"):
        packed = subprocess.check_output([str(mtype), "-i", str(images / "boot.vfat"), f"::{name}"])
        require(packed == (images / "rpi-firmware" / name).read_bytes(),
                f"Packed boot file differs: {name}")
    config = (images / "rpi-firmware/config.txt").read_text()
    require("kernel=Image" in config, "Firmware does not select Image")
    require("dtoverlay=dwc2,dr_mode=peripheral" in config, "Missing USB device overlay")
    cmdline = (images / "rpi-firmware/cmdline.txt").read_text()
    for token in ("root=/dev/mmcblk0p2", "rootwait", "init=/usr/sbin/agx-init", "logo.nologo", "console=ttyAMA10,115200",
                  "g_ether.host_addr=02:77:00:00:00:01", "g_ether.dev_addr=02:77:00:00:00:02"):
        require(token in cmdline.split(), f"Missing boot argument: {token}")
    require("console=tty1" not in cmdline.split(), "Kernel messages must not overwrite the HDMI login")
    require("disable_splash=1" in config, "Firmware splash must be disabled")
    inittab = (images.parent / "target/etc/inittab").read_text()
    for tty in ("tty1::respawn:", "ttyAMA10::respawn:"):
        require(tty in inittab, f"Missing login console: {tty}")
    target = images.parent / "target"
    require((target / "usr/sbin/dropbear").is_file(), "Missing SSH server")
    dropbear = (target / "etc/default/dropbear").read_text()
    require('-g -p 22' in dropbear and '-s' not in dropbear.split('DROPBEAR_ARGS=')[-1], "SSH must allow named-user passwords and retain key-only root")
    interfaces = (target / "etc/network/interfaces").read_text()
    require("auto usb0" in interfaces and "address 10.77.0.2" in interfaces,
            "Missing USB Ethernet address")
    kernel_config = next((images.parent / "build").glob("linux-*/.config")).read_text()
    for option in ("CONFIG_USB_GADGET=y", "CONFIG_USB_DWC2=y", "CONFIG_USB_DWC2_PERIPHERAL=y", "CONFIG_USB_ETH=y"):
        require(option in kernel_config.splitlines(), f"Missing built-in kernel option: {option}")
    require("# CONFIG_BLK_DEV_RAM is not set" in kernel_config.splitlines(),
            "Unused RAM block devices must be disabled")
    require("CONFIG_BRCMFMAC=m" in kernel_config.splitlines() and
            "CONFIG_BRCMFMAC_SDIO=y" in kernel_config.splitlines(),
            "Missing Broadcom SDIO Wi-Fi support")
    # Bluetooth: controller enabled at boot, kernel modules available, BlueZ/D-Bus, the service chain
    # and the Broadcom radio firmware.
    require("dtparam=krnbt=on" in config.splitlines(), "Bluetooth controller is not enabled")
    for option in ("CONFIG_BT=m", "CONFIG_BT_HCIUART=m"):
        require(option in kernel_config.splitlines(), f"Missing Bluetooth kernel option: {option}")
    for name in ("usr/sbin/agx-bluetoothd", "usr/libexec/bluetooth/bluetoothd", "usr/bin/dbus-daemon",
                 "usr/libexec/agx-bluetooth/run", "lib/firmware/brcm/BCM4345C0.hcd"):
        require((target / name).is_file(), f"Missing Bluetooth component: {name}")
    require((target / "usr/libexec/agx-bluetooth/run").stat().st_mode & 0o111, "Bluetooth service script is not executable")
    keys = target / "root/.ssh/authorized_keys"
    require(keys.is_file() and keys.stat().st_size > 0, "Missing authorised SSH public key")
    debugfs = images.parent / "host/sbin/debugfs"
    for name, mode in (("root/.ssh", 0o700), ("root/.ssh/authorized_keys", 0o600), ("etc/agx", 0o700), ("etc/agx/network.json", 0o600), ("etc/agx/wpa_supplicant.conf", 0o600)):
        metadata = subprocess.run([str(debugfs), "-R", f"stat /{name}", str(images / "rootfs.ext4")],
                                  capture_output=True, check=True).stdout.decode()
        actual = re.search(r"\bMode:\s+([0-7]+)\b", metadata)
        require(actual is not None and int(actual.group(1), 8) == mode,
                f"Incorrect packed SSH permissions: {name}")
        require(re.search(r"User:\s+0\s+Group:\s+0\b", metadata),
                f"Incorrect packed SSH ownership: {name}")
    for name in ("etc/inittab", "etc/network/interfaces", "etc/default/dropbear", "etc/issue", "etc/profile.d/agx-welcome.sh", "etc/init.d/S12agx-data", "usr/share/agx/logo.ascii", "root/.ssh/authorized_keys"):
        packed = subprocess.run([str(debugfs), "-R", f"cat /{name}", str(images / "rootfs.ext4")],
                                capture_output=True, check=True).stdout
        require(packed == (target / name).read_bytes(), f"Packed root filesystem differs: {name}")
    if 'BR2_PACKAGE_E2FSPROGS=y' in (images.parent / '.config').read_text().splitlines():
        require((target / 'sbin/mke2fs').is_file() and not (target / 'sbin/mke2fs').is_symlink(),
                'Real e2fsprogs mke2fs is required for ext4 data volumes')
    if 'BR2_PACKAGE_AGX=y' in (images.parent / '.config').read_text().splitlines():
        for name in ('usr/bin/agx', 'usr/sbin/agx-netd', 'usr/sbin/agx-dhcp', 'usr/sbin/agx-init'):
            binary = (target / name).read_bytes()
            require(len(binary) >= 20 and binary[:4] == b'\x7fELF' and binary[4] == 2 and binary[5] == 1,
                    f'{name} must be a 64-bit little-endian ELF')
            require(struct.unpack_from('<H', binary, 18)[0] == 183, f'{name} must target ARM64')
            packed = subprocess.run([str(debugfs), '-R', f'cat /{name}', str(images / 'rootfs.ext4')],
                                    capture_output=True, check=True).stdout
            require(packed == binary, f'Packed {name} differs from the build output')
        require('/usr/sbin/agx-init --supervise' in inittab, 'BusyBox recovery must start the Rust supervisor')
        for name in ('usr/sbin/wpa_supplicant', 'usr/sbin/iw', 'usr/bin/curl', 'etc/ssl/certs/ca-certificates.crt',
                     'lib/firmware/regulatory.db', 'lib/firmware/brcm/brcmfmac43455-sdio.raspberrypi,5-model-b.bin'):
            require((target / name).exists(), f'Missing Wi-Fi prerequisite: {name}')
        print('PASS: ARM64 AGX/TUI/network executables, startup and Wi-Fi prerequisites')
    print("PASS: ARM64 kernel, Pi 5 DTBs, boot files, consoles, SSH setup and disk partitions")
    print("Hardware boot remains unverified until tested on a Raspberry Pi 5.")


if __name__ == "__main__":
    try:
        check(Path(sys.argv[1] if len(sys.argv) > 1 else "output/images").resolve())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"FAIL: {error}")
