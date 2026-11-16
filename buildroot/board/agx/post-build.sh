#!/usr/bin/env sh
set -eu
if ! grep -q 'AGX_NETWORK' "$TARGET_DIR/etc/inittab"; then
    printf '\n# AGX_NETWORK\n::respawn:/usr/sbin/agx-netd\n' >> "$TARGET_DIR/etc/inittab"
fi
