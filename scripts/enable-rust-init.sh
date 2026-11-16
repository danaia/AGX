#!/usr/bin/env bash
# Opt the development Pi into Rust PID 1 without changing its kernel or partitions.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
RELEASE=${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}/aarch64-unknown-linux-gnu/release
scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$RELEASE/agx-init" "$DESTINATION:/usr/sbin/agx-init.new"
ssh -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$DESTINATION" 'sh -s' <<'REMOTE'
set -eu
chmod 755 /usr/sbin/agx-init.new
/usr/sbin/agx-init.new --check
mv /usr/sbin/agx-init.new /usr/sbin/agx-init
mkdir -p /run/agx-boot
mount -t vfat /dev/mmcblk0p1 /run/agx-boot
trap 'umount /run/agx-boot' EXIT
# Save the original selector once. No system image or account data is overwritten.
if ! test -f /root/agx-init-backup/cmdline.txt; then
    cp /run/agx-boot/cmdline.txt /root/agx-init-backup/cmdline.txt
fi
if grep -q 'init=' /run/agx-boot/cmdline.txt && ! grep -q 'init=/usr/sbin/agx-init' /run/agx-boot/cmdline.txt; then
    echo 'A different init is configured; refusing to override it.' >&2
    exit 1
fi
if ! grep -q 'init=/usr/sbin/agx-init' /run/agx-boot/cmdline.txt; then
    tr -d '\n' < /run/agx-boot/cmdline.txt > /run/agx-boot/cmdline.agx-new
    printf ' init=/usr/sbin/agx-init\n' >> /run/agx-boot/cmdline.agx-new
    sync
    mv /run/agx-boot/cmdline.agx-new /run/agx-boot/cmdline.txt
fi
sync
echo 'Rust PID 1 selected for the next boot. BusyBox fallback and the original boot command line are retained.'
REMOTE
