#!/usr/bin/env sh
set -eu
sed -i '/AGX_NETWORK/d; /::respawn:\/usr\/sbin\/agx-netd/d' "$TARGET_DIR/etc/inittab"
if ! grep -q 'AGX_INIT' "$TARGET_DIR/etc/inittab"; then
    printf '\n# AGX_INIT\n::respawn:/usr/sbin/agx-init --supervise\n' >> "$TARGET_DIR/etc/inittab"
fi

install -D -m 0644 "$BR2_EXTERNAL_AGX_PATH/../brand/logo-ascii.md" "$TARGET_DIR/usr/share/agx/logo.ascii"
chmod 0755 "$TARGET_DIR/usr/share/agx"

# Bluetooth service chain (D-Bus, BlueZ, agx-bluetoothd); agx-init supervises it.
install -D -m 0755 "$BR2_EXTERNAL_AGX_PATH/../apps/bluetooth/run.sh" "$TARGET_DIR/usr/libexec/agx-bluetooth/run"
