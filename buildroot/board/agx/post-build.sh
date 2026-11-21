#!/usr/bin/env sh
set -eu
sed -i '/AGX_NETWORK/d; /::respawn:\/usr\/sbin\/agx-netd/d' "$TARGET_DIR/etc/inittab"
if ! grep -q 'AGX_INIT' "$TARGET_DIR/etc/inittab"; then
    printf '\n# AGX_INIT\n::respawn:/usr/sbin/agx-init --supervise\n' >> "$TARGET_DIR/etc/inittab"
fi

install -D -m 0644 "$BR2_EXTERNAL_AGX_PATH/../brand/logo-ascii.md" "$TARGET_DIR/usr/share/agx/logo.ascii"
chmod 0755 "$TARGET_DIR/usr/share/agx"

if [ -f "$TARGET_DIR/usr/share/agx/board" ] &&
    grep -qx zero2w "$TARGET_DIR/usr/share/agx/board"; then
    # Rust init runs Dropbear directly, so Buildroot's init script never replaces this link.
    if [ -L "$TARGET_DIR/etc/dropbear" ] &&
        [ "$(readlink "$TARGET_DIR/etc/dropbear")" = /var/run/dropbear ]; then
        rm "$TARGET_DIR/etc/dropbear"
    fi
    install -d -m 0700 "$TARGET_DIR/etc/dropbear"
    install -m 0644 "$BR2_EXTERNAL_AGX_PATH/board/zero2w/ANGI-SETUP.txt" "$BINARIES_DIR/ANGI-SETUP.txt"
    if [ "${ANGI_PUBLIC_IMAGE:-0}" = 1 ]; then
        install -D -m 0600 /dev/null "$TARGET_DIR/root/.ssh/authorized_keys"
        sed -i 's/^root:[^:]*:/root:!:/' "$TARGET_DIR/etc/shadow"
    fi
    rm -f "$TARGET_DIR/etc/modules-load.d/agx-display.conf" \
        "$TARGET_DIR/etc/modules-load.d/agx-bluetooth.conf" \
        "$TARGET_DIR/usr/libexec/agx-display-access" \
        "$TARGET_DIR/usr/libexec/agx-bluetooth/run"
    exit 0
fi

# Bluetooth service chain (D-Bus, BlueZ, agx-bluetoothd); agx-init supervises it.
install -D -m 0755 "$BR2_EXTERNAL_AGX_PATH/../apps/bluetooth/run.sh" "$TARGET_DIR/usr/libexec/agx-bluetooth/run"
