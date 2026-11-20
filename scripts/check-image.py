#!/usr/bin/env python3
"""Validate ANGI image artifacts without mounting or accessing block devices."""
import argparse
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


def check(images, board):
    zero = board == "zero2w"
    model = "Raspberry Pi Zero 2 W" if zero else "Raspberry Pi 5"
    dtbs = ("bcm2710-rpi-zero-2-w.dtb",) if zero else ("bcm2712-rpi-5-b.dtb", "bcm2712d0-rpi-5-b.dtb")
    firmware = ("bootcode.bin", "start.elf", "fixup.dat", "start_cd.elf", "fixup_cd.dat",
                "overlays/disable-bt.dtbo") if zero else ()
    usb = "10.79.0.2" if zero else "10.77.0.2"
    mac_prefix = "02:79" if zero else "02:77"
    serial = "ttyAMA0" if zero else "ttyAMA10"

    for name in ("Image", *dtbs,
                 "boot.vfat", "rootfs.ext4", "sdcard.img"):
        require((images / name).is_file(), f"Missing artifact: {name}")
    with (images / "Image").open("rb") as kernel:
        kernel.seek(56)
        require(kernel.read(4) == b"ARM\x64", "Invalid ARM64 kernel header")
    for name in dtbs:
        with (images / name).open("rb") as dtb:
            require(dtb.read(4) == b"\xd0\x0d\xfe\xed", f"Invalid DTB: {name}")
            if zero:
                require(b"raspberrypi,model-zero-2-w\0" in dtb.read(), "Incorrect Zero 2 W DTB")
    with (images / "rootfs.ext4").open("rb") as rootfs:
        rootfs.seek(1080)
        require(rootfs.read(2) == b"\x53\xef", "Invalid ext filesystem magic")
    if zero:
        with (images / "boot.vfat").open("rb") as boot:
            header = boot.read(512)
        require(len(header) == 512 and header[510:] == b"\x55\xaa", "Invalid FAT boot sector")
        root_entries = struct.unpack_from("<H", header, 17)[0]
        fat16_sectors = struct.unpack_from("<H", header, 22)[0]
        fat32_sectors = struct.unpack_from("<I", header, 36)[0]
        require(root_entries == 0 and fat16_sectors == 0 and fat32_sectors > 0 and
                header[82:90] == b"FAT32   ", "Zero 2 W boot volume must be FAT32 to match the partition table")
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
    for name in ("Image", *dtbs, *firmware, "config.txt", "cmdline.txt", "overlays/dwc2.dtbo"):
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
    for token in ("root=/dev/mmcblk0p2", "rootwait", "init=/usr/sbin/agx-init", "logo.nologo", f"console={serial},115200",
                  f"g_ether.host_addr={mac_prefix}:00:00:00:01", f"g_ether.dev_addr={mac_prefix}:00:00:00:02"):
        require(token in cmdline.split(), f"Missing boot argument: {token}")
    require("console=tty1" not in cmdline.split(), "Kernel messages must not overwrite the HDMI login")
    require("disable_splash=1" in config, "Firmware splash must be disabled")
    inittab = (images.parent / "target/etc/inittab").read_text()
    for tty in ("tty1::respawn:", f"{serial}::respawn:"):
        require(tty in inittab, f"Missing login console: {tty}")
    target = images.parent / "target"
    require((target / "usr/sbin/dropbear").is_file(), "Missing SSH server")
    dropbear = (target / "etc/default/dropbear").read_text()
    require('-g -p 22' in dropbear and '-s' not in dropbear.split('DROPBEAR_ARGS=')[-1], "SSH must allow named-user passwords and retain key-only root")
    interfaces = (target / "etc/network/interfaces").read_text()
    require("auto usb0" in interfaces and f"address {usb}" in interfaces,
            "Missing USB Ethernet address")
    kernel_config = next((images.parent / "build").glob("linux-*/.config")).read_text()
    for option in ("CONFIG_USB_GADGET=y", "CONFIG_USB_DWC2=y", "CONFIG_USB_DWC2_PERIPHERAL=y", "CONFIG_USB_ETH=y"):
        require(option in kernel_config.splitlines(), f"Missing built-in kernel option: {option}")
    require("# CONFIG_BLK_DEV_RAM is not set" in kernel_config.splitlines(),
            "Unused RAM block devices must be disabled")
    require("CONFIG_BRCMFMAC=m" in kernel_config.splitlines() and
            "CONFIG_BRCMFMAC_SDIO=y" in kernel_config.splitlines(),
            "Missing Broadcom SDIO Wi-Fi support")
    if zero:
        host_keys = target / "etc/dropbear"
        require(host_keys.is_dir() and not host_keys.is_symlink(), "Missing persistent SSH host-key directory")
        require("CONFIG_ZRAM=y" in kernel_config and "CONFIG_SWAP=y" in kernel_config,
                "Missing compressed swap support")
        require("arm_64bit=1" in config.splitlines() and "dtoverlay=disable-bt" in config.splitlines(),
                "Missing Zero 2 W boot configuration")
    else:
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
    def packed_file(name):
        result = subprocess.run([str(debugfs), "-R", f"cat /{name}", str(images / "rootfs.ext4")],
                                capture_output=True, check=True)
        return result.stdout

    permissions = [("root/.ssh", 0o700), ("root/.ssh/authorized_keys", 0o600), ("etc/agx", 0o700),
                   ("etc/agx/network.json", 0o600), ("etc/agx/wpa_supplicant.conf", 0o600)]
    if zero:
        permissions.extend([("etc/agx/gui.env", 0o600), ("etc/dropbear", 0o700),
                            ("var/lib/agx-data/agx-opencode-users", 0o1777)])
    for name, mode in permissions:
        metadata = subprocess.run([str(debugfs), "-R", f"stat /{name}", str(images / "rootfs.ext4")],
                                  capture_output=True, check=True).stdout.decode()
        actual = re.search(r"\bMode:\s+([0-7]+)\b", metadata)
        require(actual is not None and int(actual.group(1), 8) == mode,
                f"Incorrect packed SSH permissions: {name}")
        require(re.search(r"User:\s+0\s+Group:\s+0\b", metadata),
                f"Incorrect packed SSH ownership: {name}")
        if name == "etc/dropbear":
            require("Type: directory" in metadata, "Packed SSH host-key directory must be on the SD root filesystem")
    for name in ("etc/inittab", "etc/network/interfaces", "etc/default/dropbear", "etc/issue", "etc/profile.d/agx-welcome.sh", "etc/init.d/S12agx-data", "usr/share/agx/logo.ascii", "root/.ssh/authorized_keys"):
        require(packed_file(name) == (target / name).read_bytes(), f"Packed root filesystem differs: {name}")
    if zero:
        wifi_module_options = packed_file("etc/modprobe.d/angi-zero-wifi.conf")
        require(b"options brcmfmac feature_disable=0x2000" in wifi_module_options.splitlines(),
                "Zero 2 W must handle the WPA2 handshake in wpa_supplicant")
    if 'BR2_PACKAGE_E2FSPROGS=y' in (images.parent / '.config').read_text().splitlines():
        require((target / 'sbin/mke2fs').is_file() and not (target / 'sbin/mke2fs').is_symlink(),
                'Real e2fsprogs mke2fs is required for ext4 data volumes')
    if 'BR2_PACKAGE_AGX=y' in (images.parent / '.config').read_text().splitlines():
        binaries = ('usr/bin/agx', 'usr/sbin/agx-netd', 'usr/sbin/agx-dhcp', 'usr/sbin/agx-init')
        if zero:
            binaries += ('usr/bin/agx-gui', 'usr/libexec/opencode/opencode')
        for name in binaries:
            binary = (target / name).read_bytes()
            require(len(binary) >= 20 and binary[:4] == b'\x7fELF' and binary[4] == 2 and binary[5] == 1,
                    f'{name} must be a 64-bit little-endian ELF')
            require(struct.unpack_from('<H', binary, 18)[0] == 183, f'{name} must target ARM64')
            require(packed_file(name) == binary, f'Packed {name} differs from the build output')
        require('/usr/sbin/agx-init --supervise' in inittab, 'BusyBox recovery must start the Rust supervisor')
        wifi = ('lib/firmware/brcm/brcmfmac43430-sdio.raspberrypi,model-zero-2-w.bin',
                'lib/firmware/brcm/brcmfmac43436-sdio.raspberrypi,model-zero-2-w.bin') if zero else (
                'lib/firmware/brcm/brcmfmac43455-sdio.raspberrypi,5-model-b.bin',)
        for name in ('usr/sbin/wpa_supplicant', 'usr/sbin/iw', 'usr/bin/curl', 'etc/ssl/certs/ca-certificates.crt',
                     'lib/firmware/regulatory.db', *wifi):
            require((target / name).exists(), f'Missing Wi-Fi prerequisite: {name}')
        print('PASS: ARM64 AGX/TUI/network executables, startup and Wi-Fi prerequisites')
    if zero:
        require((images / "rootfs.ext4").stat().st_size == 8 * 1024**3, "Unexpected root partition size")
        require(packed_file("usr/share/agx/board") == b"zero2w\n", "Missing Zero board identifier")
        require(packed_file("etc/agx/gui.env") == b"SLINT_BACKEND=vnc-software\nAGX_GUI_VNC=on\n",
                "Missing headless VNC defaults")
        for name in ("usr/bin/agent", "usr/bin/opencode", "usr/libexec/agx-agent/project.py"):
            require((target / name).is_file() and packed_file(name) == (target / name).read_bytes(),
                    f"Missing packed workspace component: {name}")
        for name in ("usr/bin/herdr", "usr/libexec/agx-web", "usr/share/agx/agent/work-editor", "usr/bin/angi-build"):
            require(not (target / name).exists(), f"Unexpected package component: {name}")
        require(not any((target / "usr/lib").glob("*[Ww]eb[Kk]it*")), "Unexpected WebKit library")
        require(not list((target / "etc/dropbear").glob("*host_key")), "Image contains an SSH host key")
        with (images / "sdcard.img").open("rb") as disk:
            checksum = hashlib.file_digest(disk, "sha256").hexdigest()
        (images / "sdcard.img.sha256").write_text(f"{checksum}  sdcard.img\n")
    print(f"PASS: {model} ARM64 kernel, DTBs, boot files, consoles, SSH setup and disk partitions")
    print(f"Hardware boot requires testing on a {model}.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("images", nargs="?", type=Path)
    parser.add_argument("--board", choices=("pi5", "zero2w"), default="pi5")
    args = parser.parse_args()
    images = args.images or Path("output/zero2w/images" if args.board == "zero2w" else "output/images")
    try:
        check(images.resolve(), args.board)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"FAIL: {error}")
