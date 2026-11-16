#!/usr/bin/env bash
# Restore PID 1's SD-resident libgcc after the first GPU runtime deployment.
# Usage: sudo scripts/repair-gui-boot.sh /path/to/mounted/agx-rootfs
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TARGET_ROOT=${1:?Pass the mounted AGX SD root filesystem}
LIBRARY="$ROOT/output/target/lib/libgcc_s.so.1"
[[ $(id -u) == 0 ]] || { echo 'Root access is required to repair the mounted SD filesystem.' >&2; exit 1; }
[[ -f "$TARGET_ROOT/usr/sbin/agx-init" && -f "$TARGET_ROOT/etc/hostname" ]] || {
    echo 'This does not look like the AGX root filesystem.' >&2; exit 1;
}
[[ $(cat "$TARGET_ROOT/etc/hostname") == agx ]] || { echo 'Expected the AGX filesystem.' >&2; exit 1; }
mountpoint -q "$TARGET_ROOT" || { echo 'Pass a mounted filesystem, not a directory.' >&2; exit 1; }
[[ -f "$LIBRARY" && ! -L "$LIBRARY" ]] || { echo 'Missing original Buildroot libgcc.' >&2; exit 1; }
readelf -h "$LIBRARY" | rg -q 'Machine:.*AArch64' || { echo 'Expected an ARM64 library.' >&2; exit 1; }
install -m 755 "$LIBRARY" "$TARGET_ROOT/lib/libgcc_s.so.1.agx-repair"
mv -f "$TARGET_ROOT/lib/libgcc_s.so.1.agx-repair" "$TARGET_ROOT/lib/libgcc_s.so.1"
sync
cmp "$LIBRARY" "$TARGET_ROOT/lib/libgcc_s.so.1"
echo 'Restored libgcc on the SD root. Unmount the card before returning it to the Pi.'
