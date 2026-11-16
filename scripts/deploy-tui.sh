#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
BINARY="${AGX_BINARY:-${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}/aarch64-unknown-linux-gnu/release/agx}"
if [[ ! -f "$BINARY" ]]; then
    echo 'Run make tui-build before deployment.' >&2
    exit 1
fi
# Dropbear includes legacy SCP, rather than an SFTP subsystem.
scp -O -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$BINARY" "$DESTINATION:/usr/bin/agx.new"
ssh -o BatchMode=yes -o ConnectTimeout=10 -i "$KEY" "$DESTINATION" \
    'chmod 755 /usr/bin/agx.new && /usr/bin/agx.new --version && { test ! -f /usr/bin/agx || cp /usr/bin/agx /usr/bin/agx.prev; } && mv /usr/bin/agx.new /usr/bin/agx'
