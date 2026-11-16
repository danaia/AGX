#!/usr/bin/env bash
# First transition under BusyBox PID 1. Booting Rust PID 1 is a separate, verified step.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
RELEASE=${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}/aarch64-unknown-linux-gnu/release
for binary in agx-init agx-netd; do
    scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$RELEASE/$binary" "$DESTINATION:/usr/sbin/$binary.new"
done
AGX_BINARY="$RELEASE/agx" "$ROOT/scripts/deploy-tui.sh"
ssh -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$DESTINATION" 'sh -s' <<'REMOTE'
set -eu
install -d -m 700 /root/agx-init-backup
if test -f /run/agx/init.pid && kill -0 "$(cat /run/agx/init.pid)" 2>/dev/null; then
    echo 'AGX init is already running; use a controlled reboot to replace its executable.' >&2
    exit 1
fi
cp /etc/inittab /root/agx-init-backup/inittab
cp /usr/sbin/agx-netd /root/agx-init-backup/agx-netd
chmod 755 /usr/sbin/agx-init.new /usr/sbin/agx-netd.new
/usr/sbin/agx-init.new --check
mv /usr/sbin/agx-init.new /usr/sbin/agx-init
mv /usr/sbin/agx-netd.new /usr/sbin/agx-netd
old_pid=$(cat /run/agx/network.pid)
sed '/AGX_NETWORK/d; /::respawn:\/usr\/sbin\/agx-netd/d' /etc/inittab > /etc/inittab.agx-new
printf '\n# AGX_INIT\n::respawn:/usr/sbin/agx-init --supervise\n' >> /etc/inittab.agx-new
mv /etc/inittab.agx-new /etc/inittab
# Reload first so BusyBox no longer respawns the old network service.
kill -HUP 1
sleep 1
if test -r "/proc/$old_pid/comm" && test "$(cat /proc/$old_pid/comm)" = agx-netd; then
    kill -TERM "$old_pid"
fi
sync
REMOTE
for attempt in {1..20}; do
    if ssh -o BatchMode=yes -o ConnectTimeout=5 -i "$KEY" "$DESTINATION" 'agx services --json' ; then
        exit 0
    fi
    sleep 1
done
echo 'Service manager unavailable. Original inittab and network binary are in /root/agx-init-backup.' >&2
exit 1
