#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
RELEASE="${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}/aarch64-unknown-linux-gnu/release"
export AGX_RELEASE_DIR="$RELEASE"
python3 - <<'PY'
from pathlib import Path
import glob, os, tarfile
root=Path('output/target')
release=Path(os.environ['AGX_RELEASE_DIR'])
def root_owner(entry):
    entry.uid=entry.gid=0
    entry.uname=entry.gname='root'
    return entry
with tarfile.open('.cache/network-stack.tar','w') as archive:
    for name in ['lib/firmware','lib/modules','etc/ssl','usr/share/ca-certificates','usr/sbin/wpa_supplicant','usr/sbin/wpa_cli','usr/sbin/iw','usr/bin/curl']:
        path=root/name
        if not path.exists(): raise SystemExit(f'Missing network prerequisite: {path}; run make build')
        archive.add(path,arcname=name,filter=root_owner)
    for folder in ['usr/lib','lib']:
        for pattern in ['libssl.so*','libcrypto.so*','libnl*.so*','libcurl.so*','libz.so*','libatomic.so*']:
            for name in glob.glob(str(root/folder/pattern)):
                archive.add(name,arcname=str(Path(name).relative_to(root)),recursive=False,filter=root_owner)
    archive.add('buildroot/board/agx/rootfs-overlay/etc/default/dropbear',arcname='etc/default/dropbear.agx-new',filter=root_owner)
    # Keep the running init/shell BusyBox inode untouched; only add its NTP applet.
    archive.add(root/'bin/busybox',arcname='usr/libexec/agx/busybox',filter=root_owner)
    for binary in ['agx-netd','agx-dhcp']:
        archive.add(release/binary,arcname=f'usr/sbin/{binary}.new',filter=root_owner)
PY
KERNEL=$(cat output/build/linux-custom/include/config/kernel.release)
REMOTE_KERNEL=$(ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" uname -r)
[[ "$KERNEL" == "$REMOTE_KERNEL" ]] || { echo 'Kernel mismatch: refusing module deployment' >&2; exit 1; }
scp -O -o BatchMode=yes -i "$KEY" .cache/network-stack.tar "$DESTINATION:/tmp/agx-network-stack.tar"
ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" 'sh -s' <<'REMOTE'
set -eu
old_pid=$(cat /run/agx/network.pid 2>/dev/null || true)
tar -xf /tmp/agx-network-stack.tar -C /
chmod 755 /usr/sbin/agx-netd.new /usr/sbin/agx-dhcp.new /usr/libexec/agx/busybox
mv /usr/sbin/agx-netd.new /usr/sbin/agx-netd
mv /usr/sbin/agx-dhcp.new /usr/sbin/agx-dhcp
ln -sf ../libexec/agx/busybox /usr/sbin/ntpd
/sbin/depmod -a
if ! grep -q 'DROPBEAR_ARGS="-s -p 22"' /etc/default/dropbear; then
    cp /etc/default/dropbear /etc/default/dropbear.agx-prev
    mv /etc/default/dropbear.agx-new /etc/default/dropbear
    /etc/init.d/S50dropbear restart
fi
if ! grep -q 'AGX_NETWORK' /etc/inittab; then
    printf '\n# AGX_NETWORK\n::respawn:/usr/sbin/agx-netd\n' >> /etc/inittab
fi
if test -n "$old_pid" && test -r /proc/$old_pid/comm && test "$(cat /proc/$old_pid/comm)" = agx-netd; then
    kill -TERM "$old_pid"
fi
kill -HUP 1
REMOTE
for attempt in {1..20}; do
    if ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" 'agx wifi status --json >/dev/null 2>&1 && /usr/sbin/wpa_cli -p /run/agx/wpa -i wlan0 ping | grep -q PONG'; then
        echo 'Network service ready.'
        exit 0
    fi
    sleep 1
done
echo 'Network service did not become ready; inspect its status before provisioning.' >&2
exit 1
