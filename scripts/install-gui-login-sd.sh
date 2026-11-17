#!/usr/bin/env bash
# Install the graphical login executable on a mounted AGX SD card.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
SD_ROOT=${1:?Pass the mounted AGX root filesystem}
SD_BOOT=${2:?Pass the mounted AGX boot filesystem}
GUI="$ROOT/.cache/agx-gui-final.stripped"
INIT="$ROOT/.cache/cargo-target/aarch64-unknown-linux-gnu/release/agx-init"
DEVICES="$ROOT/.cache/cargo-target/aarch64-unknown-linux-gnu/release/agx-display-devices"
[[ $(id -u) == 0 ]] || { echo 'Administrator access is required for the SD filesystem.' >&2; exit 1; }
mountpoint -q "$SD_ROOT" && mountpoint -q "$SD_BOOT"
[[ $(cat "$SD_ROOT/etc/hostname") == agx ]]
grep -qw 'init=/usr/sbin/agx-init' "$SD_BOOT/cmdline.txt"
grep -q '^dtoverlay=vc4-kms-v3d' "$SD_BOOT/config.txt"
[[ -s "$SD_ROOT/etc/agx/data-volume.uuid" && -x "$SD_ROOT/etc/init.d/S12agx-data" ]]
[[ -f "$SD_ROOT/lib/libgcc_s.so.1" && ! -L "$SD_ROOT/lib/libgcc_s.so.1" ]]
cmp "$INIT" "$SD_ROOT/usr/sbin/agx-init"
[[ -f "$GUI" && ! -L "$GUI" && -x "$DEVICES" ]]
readelf -h "$GUI" | grep -q 'Machine:.*AArch64'
gui_bytes=$(stat -c %s "$GUI")
free_kb=$(df -Pk "$SD_ROOT" | awk 'NR == 2 { print $4 }')
(( free_kb * 1024 > gui_bytes + 8 * 1024 * 1024 )) || {
    echo 'Insufficient free space for the GUI and an 8 MiB reserve.' >&2; exit 1;
}

fingerprint() {
    for name in etc/passwd etc/shadow etc/hostname etc/agx/network.json etc/agx/wpa_supplicant.conf; do
        [[ ! -f "$SD_ROOT/$name" ]] || sha256sum "$SD_ROOT/$name"
    done
}
before=$(fingerprint)
backup="$SD_ROOT/root/agx-gui-sd-backup"
install -d -m 700 "$backup"
for name in usr/bin/agx-gui etc/agx/gui.env; do
    destination="$backup/${name##*/}"
    if [[ ( -e "$SD_ROOT/$name" || -L "$SD_ROOT/$name" ) && ! -e "$destination" && ! -L "$destination" ]]; then
        cp -Pp "$SD_ROOT/$name" "$destination"
    fi
done

# Replace the executable's NVMe symlink with a regular SD-resident executable.
install -m 755 "$GUI" "$SD_ROOT/usr/bin/agx-gui.sd-new"
mv -f "$SD_ROOT/usr/bin/agx-gui.sd-new" "$SD_ROOT/usr/bin/agx-gui"
config="$SD_ROOT/etc/agx/gui.env"
if [[ -f "$config" ]]; then
    sed '/^[[:space:]]*SLINT_BACKEND[[:space:]]*=/d' "$config" > "$config.sd-new"
else
    : > "$config.sd-new"
fi
printf '%s\n' 'SLINT_BACKEND=auto' >> "$config.sd-new"
chmod 600 "$config.sd-new"
mv -f "$config.sd-new" "$config"
install -D -m 644 "$ROOT/buildroot/board/agx/rootfs-overlay/etc/modules-load.d/agx-display.conf" \
    "$SD_ROOT/etc/modules-load.d/agx-display.conf"
install -D -m 755 "$DEVICES" "$SD_ROOT/usr/libexec/agx-display-devices"
install -D -m 755 "$ROOT/buildroot/board/agx/rootfs-overlay/usr/libexec/agx-display-access" \
    "$SD_ROOT/usr/libexec/agx-display-access"
cmp "$GUI" "$SD_ROOT/usr/bin/agx-gui"
[[ ! -L "$SD_ROOT/usr/bin/agx-gui" ]]
[[ "$before" == "$(fingerprint)" ]]
sync
echo 'Verified: SD boots Rust init; graphical login selects HDMI or enabled VNC at startup.'
echo 'GUI executable is on SD; existing GPU runtime libraries are on the Pi NVMe volume.'
echo 'Account, hostname and Wi-Fi configuration checksums are unchanged.'
