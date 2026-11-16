#!/usr/bin/env bash
# Install account provisioning over the existing root-key USB recovery connection.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
export AGX_RELEASE_DIR="${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}/aarch64-unknown-linux-gnu/release"
python3 - <<'PY'
from pathlib import Path
import os, tarfile
release=Path(os.environ['AGX_RELEASE_DIR'])
def owner(entry):
    entry.uid=entry.gid=0
    entry.uname=entry.gname='root'
    return entry
with tarfile.open('.cache/profile-stack.tar','w') as archive:
    for binary,target in [('agx','usr/bin/agx.new'),('agx-netd','usr/sbin/agx-netd.new')]:
        archive.add(release/binary,arcname=target,filter=owner)
    overlay=Path('buildroot/board/agx/rootfs-overlay')
    for name in ['etc/default/dropbear','etc/issue','etc/profile.d/agx-welcome.sh']:
        archive.add(overlay/name,arcname=name+'.agx-new',filter=owner)
    archive.add('brand/logo-ascii.md',arcname='usr/share/agx/logo.ascii',filter=owner)
PY
scp -O -o BatchMode=yes -i "$KEY" .cache/profile-stack.tar "$DESTINATION:/tmp/agx-profile-stack.tar"
ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" 'sh -s' <<'REMOTE'
set -eu
tar -xf /tmp/agx-profile-stack.tar -C /
chmod 755 /usr/bin/agx.new /usr/sbin/agx-netd.new
/usr/bin/agx.new --version
cp /usr/bin/agx /usr/bin/agx.prev
cp /usr/sbin/agx-netd /usr/sbin/agx-netd.prev
mv /usr/bin/agx.new /usr/bin/agx
mv /usr/sbin/agx-netd.new /usr/sbin/agx-netd
for name in /etc/default/dropbear /etc/issue /etc/profile.d/agx-welcome.sh; do
    chmod 644 "$name.agx-new"
    mv "$name.agx-new" "$name"
done
chmod 755 /usr/share/agx
chmod 644 /usr/share/agx/logo.ascii
pid=$(cat /run/agx/network.pid)
test "$(cat /proc/$pid/comm)" = agx-netd
kill -TERM "$pid"
/etc/init.d/S50dropbear restart
REMOTE
for attempt in {1..25}; do
    if ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" 'agx wifi status --json >/dev/null'; then
        echo 'Profile service ready; root SSH key recovery verified.'
        exit 0
    fi
    sleep 1
done
echo 'Service readiness failed; inspect the USB recovery shell.' >&2
exit 1
