#!/usr/bin/env bash
# Install the desktop (agx-gui) on a running AGX Pi without reflashing: the
# runtime libraries and data from the Buildroot target tree, the binary, and
# the HDMI display overlay in config.txt. Run `make deploy-gui` to build all binaries.
# Prerequisite: the packages in agx_rpi5_defconfig's "Desktop" section are built.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
TARGET="$ROOT/output/target"
BINARY="${AGX_GUI_BINARY:-${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}/aarch64-unknown-linux-gnu/release/agx-gui}"
RELEASE="${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}/aarch64-unknown-linux-gnu/release"
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$DESTINATION")
[[ -f "$BINARY" ]] || { echo 'Run make gui-build before deployment.' >&2; exit 1; }
for binary in agx agx-init; do
    [[ -f "$RELEASE/$binary" ]] || { echo 'Run make init-build before deployment.' >&2; exit 1; }
done
[[ -d "$TARGET/usr/share/X11/xkb" ]] || { echo 'Build the desktop packages first (see defconfig).' >&2; exit 1; }

# Keep the first installed versions so recovery SSH can restore the whole stack.
"${SSH[@]}" 'set -e
mkdir -p /root/agx-gui-backup
for binary in /usr/bin/agx /usr/bin/agx-gui /usr/sbin/agx-init; do
    if test -f "$binary" && ! test -f "/root/agx-gui-backup/${binary##*/}"; then
        cp -p "$binary" "/root/agx-gui-backup/${binary##*/}"
    fi
done'

STRIPPED="$ROOT/.cache/agx-gui.stripped"
"$ROOT/output/host/bin/aarch64-linux-strip" -o "$STRIPPED" "$BINARY"

echo "Copying runtime libraries and data to $DESTINATION"
(cd "$TARGET" && tar --owner=0 --group=0 --numeric-owner -cf - \
    usr/lib/libevdev.so.* usr/lib/libinput.so.* usr/lib/libudev.so.* \
    usr/lib/libxkbcommon.so.* usr/lib/libfontconfig.so.* usr/lib/libfreetype.so.* \
    usr/lib/libexpat.so.* usr/share/libinput usr/share/X11 usr/share/fontconfig etc/fonts) |
    "${SSH[@]}" 'tar -xf - -C /'

echo 'Staging desktop, graphical login and startup binaries'
scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$STRIPPED" "$DESTINATION:/usr/bin/agx-gui.new"
scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$RELEASE/agx" "$DESTINATION:/usr/bin/agx.new"
scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$RELEASE/agx-init" "$DESTINATION:/usr/sbin/agx-init.new"
"${SSH[@]}" 'mkdir -p /usr/libexec'
scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$ROOT/buildroot/board/agx/rootfs-overlay/usr/libexec/agx-display-access" "$DESTINATION:/usr/libexec/agx-display-access.new"
"${SSH[@]}" 'set -e
chmod 755 /usr/bin/agx-gui.new /usr/bin/agx.new /usr/sbin/agx-init.new /usr/libexec/agx-display-access.new
/usr/bin/agx.new --version
/usr/sbin/agx-init.new --check
libraries=$(ldd /usr/bin/agx-gui.new 2>&1) || { printf "%s\n" "$libraries"; exit 1; }
if printf "%s\n" "$libraries" | grep -q "not found"; then
    printf "%s\n" "$libraries" >&2; exit 1
fi
mv /usr/bin/agx.new /usr/bin/agx
mv /usr/libexec/agx-display-access.new /usr/libexec/agx-display-access
mv /usr/bin/agx-gui.new /usr/bin/agx-gui
mv /usr/sbin/agx-init.new /usr/sbin/agx-init'

# fontconfig finds no fonts on the bare image, and Slint's text stack panics on that.
echo 'Installing a system font'
"${SSH[@]}" 'mkdir -p /usr/share/fonts'
scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$ROOT/crates/agx-gui/ui/fonts/JetBrainsMonoNerdFont-Regular.ttf" "$DESTINATION:/usr/share/fonts/"

echo 'Checking the display overlay in config.txt'
"${SSH[@]}" 'set -e
mkdir -p /mnt/agxboot && mount /dev/mmcblk0p1 /mnt/agxboot
trap "umount /mnt/agxboot" EXIT
if grep -q "^dtoverlay=vc4-kms-v3d" /mnt/agxboot/config.txt; then
    echo "vc4-kms-v3d already enabled"
else
    test -f /mnt/agxboot/config.txt.pre-gui || cp /mnt/agxboot/config.txt /mnt/agxboot/config.txt.pre-gui
    printf "\n# HDMI display through DRM/KMS for the desktop.\ndtoverlay=vc4-kms-v3d\n" >> /mnt/agxboot/config.txt
    echo "Added dtoverlay=vc4-kms-v3d (backup: config.txt.pre-gui). Reboot to apply."
fi
sync'
echo 'Installed. Reboot to start directly at graphical login; SSH and serial recovery remain available.'
