#!/usr/bin/env sh
set -eu
if ! grep -q 'AGX_NETWORK' "$TARGET_DIR/etc/inittab"; then
    printf '\n# AGX_NETWORK\n::respawn:/usr/sbin/agx-netd\n' >> "$TARGET_DIR/etc/inittab"
fi

install -D -m 0644 "$BR2_EXTERNAL_AGX_PATH/../brand/logo-ascii.md" "$TARGET_DIR/usr/share/agx/logo.ascii"
chmod 0755 "$TARGET_DIR/usr/share/agx"
