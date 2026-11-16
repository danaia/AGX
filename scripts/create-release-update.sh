#!/usr/bin/env bash
# Build a release candidate without embedding the workstation's recovery key.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
RELEASE_VERSION=${AGX_RELEASE_VERSION:-v0.4.0-preview.1}
[[ "$RELEASE_VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]] || { echo 'Invalid release version.' >&2; exit 1; }
if [[ -n "$(git status --porcelain)" ]]; then
    echo 'Commit or stash repository changes before producing a release candidate.' >&2
    exit 1
fi
RELEASE_COMMIT=$(git rev-parse HEAD)
make build
make check-image
mkdir -p .cache/release-update
cp --reflink=auto output/images/rootfs.ext4 .cache/release-update/rootfs.ext4
# Unlinking alone leaves deleted file bytes in the filesystem image. Zero the
# key's data blocks first so the candidate contains no recoverable key text.
python3 - <<'PYBLOCK'
import os, re, subprocess
from pathlib import Path
image=Path('.cache/release-update/rootfs.ext4')
debugfs='output/host/sbin/debugfs'
blocks=subprocess.check_output([debugfs,'-R','blocks /root/.ssh/authorized_keys',str(image)],stderr=subprocess.DEVNULL,text=True).split()
stats=subprocess.check_output([debugfs,'-R','stats',str(image)],stderr=subprocess.DEVNULL,text=True)
match=re.search(r'^Block size:\s+(\d+)',stats,re.MULTILINE)
if not blocks or not match or not all(block.isdecimal() for block in blocks):
    raise SystemExit('Could not locate the workstation key blocks for redaction.')
size=int(match.group(1))
with image.open('r+b') as target:
    for block in blocks:
        target.seek(int(block)*size)
        target.write(bytes(size))
    target.flush()
    os.fsync(target.fileno())
PYBLOCK
output/host/sbin/debugfs -w -R 'rm /root/.ssh/authorized_keys' .cache/release-update/rootfs.ext4 >.cache/release-update/key-removal.log 2>&1
if output/host/sbin/debugfs -R 'stat /root/.ssh/authorized_keys' .cache/release-update/rootfs.ext4 2>/dev/null | grep -q 'Inode:'; then
    echo 'Workstation recovery key still exists in the release filesystem.' >&2
    exit 1
fi
if output/host/sbin/debugfs -R 'stat /etc/agx/github-token' .cache/release-update/rootfs.ext4 2>/dev/null | grep -q 'Inode:'; then
    echo 'A GitHub token is present in the release filesystem.' >&2
    exit 1
fi
if output/host/sbin/debugfs -R 'cat /etc/agx/wpa_supplicant.conf' .cache/release-update/rootfs.ext4 2>/dev/null | grep -q 'network[[:space:]]*='; then
    echo 'A saved network is present in the release filesystem.' >&2
    exit 1
fi
AGX_RELEASE_COMMIT="$RELEASE_COMMIT" AGX_RELEASE_VERSION="$RELEASE_VERSION" AGX_UPDATE_ROOTFS="$ROOT/.cache/release-update/rootfs.ext4" python3 scripts/create-system-update.py
(cd output/updates && sha256sum agx-system-update.tar.gz > agx-system-update.tar.gz.sha256)
echo "Release candidate $RELEASE_VERSION prepared."
