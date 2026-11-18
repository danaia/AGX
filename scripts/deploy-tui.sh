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
SSH_OPTIONS=(-o BatchMode=yes -o ConnectTimeout=10 -i "$KEY")
if [[ -n "${AGX_SSH_HOST_KEY_ALIAS:-}" ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)
fi
# Dropbear includes legacy SCP, rather than an SFTP subsystem.
scp -O "${SSH_OPTIONS[@]}" "$BINARY" "$DESTINATION:/usr/bin/agx.new"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'set -eu
    backup=/usr/bin
    if mountpoint -q /var/lib/agx-data; then
        backup=/var/lib/agx-data/rollback-backups/agx-tui
        install -d -m 700 /var/lib/agx-data/rollback-backups "$backup"
    fi
    chmod 755 /usr/bin/agx.new
    /usr/bin/agx.new --version
    if test -f /usr/bin/agx; then
        cp -p /usr/bin/agx "$backup/agx.prev.new"
        mv -f "$backup/agx.prev.new" "$backup/agx.prev"
    fi
    mv /usr/bin/agx.new /usr/bin/agx'
