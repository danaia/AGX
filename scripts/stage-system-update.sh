#!/usr/bin/env bash
# Transfer a workstation-built candidate to the Pi over the existing root SSH key.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
PACKAGE=${AGX_UPDATE_PACKAGE:-"$ROOT/output/updates/agx-system-update.tar.gz"}
SSH_OPTIONS=(-i "$KEY" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=yes)
if [[ -n ${AGX_SSH_HOST_KEY_ALIAS:-} ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS")
fi
pi() { ssh "${SSH_OPTIONS[@]}" "$DESTINATION" "$@"; }

[[ -f "$PACKAGE" ]] || { echo 'Run make system-update-package first.' >&2; exit 1; }
EXPECTED=$(sha256sum "$PACKAGE" | cut -d ' ' -f 1)
REMOTE_DIR=$(pi 'set -eu
mountpoint -q /var/lib/agx-data || { echo "Mount the ANGI data volume before staging an update." >&2; exit 1; }
install -d -m 750 -o root -g agx-admin /var/lib/agx-data/agx-update
mktemp -d /var/lib/agx-data/agx-update/.stage.XXXXXXXX')
[[ $REMOTE_DIR =~ ^/var/lib/agx-data/agx-update/\.stage\.[a-zA-Z0-9]+$ ]]
trap 'pi "test ! -e $REMOTE_DIR/package || unlink $REMOTE_DIR/package; rmdir $REMOTE_DIR" >/dev/null 2>&1 || true' EXIT
scp -O "${SSH_OPTIONS[@]}" "$PACKAGE" "$DESTINATION:$REMOTE_DIR/package"
pi 'sh -s' -- "$REMOTE_DIR" "$EXPECTED" <<'REMOTE'
set -eu
mountpoint -q /var/lib/agx-data
actual=$(sha256sum "$1/package" | cut -d ' ' -f 1)
test "$actual" = "$2" || { echo 'Transfer digest mismatch.' >&2; exit 1; }
chown root:agx-admin "$1/package"
chmod 640 "$1/package"
sync
mv -fT "$1/package" /var/lib/agx-data/agx-update/candidate.tar.gz
sync
REMOTE
echo 'Candidate staged. Open System and choose Check for updates.'
