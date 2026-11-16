#!/usr/bin/env bash
# Transfer a workstation-built candidate to the Pi over the existing root SSH key.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
PACKAGE=${AGX_UPDATE_PACKAGE:-"$ROOT/output/updates/agx-system-update.tar.gz"}
[[ -f "$PACKAGE" ]] || { echo 'Run make system-update-package first.' >&2; exit 1; }
EXPECTED=$(sha256sum "$PACKAGE" | cut -d ' ' -f 1)
REMOTE_DIR=$(ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" 'umask 077; mktemp -d /tmp/agx-update.XXXXXXXX')
trap 'ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" "rm -f $REMOTE_DIR/package; rmdir $REMOTE_DIR" >/dev/null 2>&1 || true' EXIT
scp -O -o BatchMode=yes -i "$KEY" "$PACKAGE" "$DESTINATION:$REMOTE_DIR/package"
ssh -o BatchMode=yes -i "$KEY" "$DESTINATION" 'sh -s' -- "$REMOTE_DIR" "$EXPECTED" <<'REMOTE'
set -eu
actual=$(sha256sum "$1/package" | cut -d ' ' -f 1)
test "$actual" = "$2" || { echo 'Transfer digest mismatch.' >&2; exit 1; }
install -d -m 750 -o root -g agx-admin /var/lib/agx-update
cp "$1/package" /var/lib/agx-update/candidate.tar.gz.new
chown root:agx-admin /var/lib/agx-update/candidate.tar.gz.new
chmod 640 /var/lib/agx-update/candidate.tar.gz.new
sync
mv /var/lib/agx-update/candidate.tar.gz.new /var/lib/agx-update/candidate.tar.gz
sync
REMOTE
echo 'Candidate staged. Open System and choose Check for updates.'
