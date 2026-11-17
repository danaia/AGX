#!/usr/bin/env bash
# Install the isolated Web runtime onto mounted NVMe without changing boot libraries.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
SSH_OPTIONS=(-o BatchMode=yes -o ConnectTimeout=10 -i "$KEY")
if [[ -n "${AGX_SSH_HOST_KEY_ALIAS:-}" ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)
fi
ARCHIVE="$ROOT/.cache/agx-web-runtime.tar"
BINARY=${AGX_GUI_BINARY:-"$ROOT/.cache/cargo-target/aarch64-unknown-linux-gnu/release/agx-gui"}
if [[ ! -f "$BINARY" ]]; then
    echo 'Run make gui-build before deploying the Web app.' >&2; exit 1
fi
python3 - "$BINARY" <<'PY_CHECK_GUI'
from pathlib import Path
import sys
if b"--prepare-web-storage" not in Path(sys.argv[1]).read_bytes():
    raise SystemExit("Rebuild the GUI with make gui-build before deploying Web.")
PY_CHECK_GUI
python3 "$ROOT/scripts/package-web-runtime.py" "$ROOT/output/target" "$ROOT/.cache/agx-web/agx-web" "$ARCHIVE"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'mountpoint -q /var/lib/agx-data && mkdir -p /var/lib/agx-data/agx-web'
STRIPPED="$ROOT/.cache/agx-web/agx-gui"
"$ROOT/output/host/bin/aarch64-linux-strip" -o "$STRIPPED" "$BINARY"
scp -O "${SSH_OPTIONS[@]}" "$STRIPPED" "$DESTINATION:/var/lib/agx-data/agx-web/gui.new"
scp -O "${SSH_OPTIONS[@]}" "$ARCHIVE" "$DESTINATION:/var/lib/agx-data/agx-web/runtime.tar"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'set -e
umask 022
archive=/var/lib/agx-data/agx-web/runtime.tar
release_id=$(sha256sum "$archive" | cut -c1-16)
release=/var/lib/agx-data/agx-web/releases/$release_id
if ! test -d "$release"; then
    stage=$release.new
    rm -rf "$stage"
    mkdir -p "$stage"
    tar -xf "$archive" -C "$stage"
    touch "$stage/.complete"
    mv -T "$stage" "$release"
elif ! test -f "$release/.complete"; then
    echo "Incomplete Web release: $release" >&2; exit 1
fi
# Prepare stores with the same fd-based checks used by the root greeter.
/var/lib/agx-data/agx-web/gui.new --prepare-web-storage
# Preserve original destinations once; each file switches atomically.
backup=/var/lib/agx-data/agx-web/backup
# Existing SSH sessions can use umask 077; runtime directories must be traversable.
find "$release/usr" "$release/etc" -type d | while IFS= read -r source; do
    destination=/${source#"$release"/}
    mkdir -p "$destination"
    chmod 755 "$destination"
done
find "$release/usr" "$release/etc" \( -type f -o -type l \) | while IFS= read -r source; do
    relative=${source#"$release"/}
    case "$relative" in usr/*|etc/pkcs11/*|etc/ssl/certs/ca-certificates.crt) ;; *) echo "Unexpected runtime path: $relative" >&2; exit 1 ;; esac
    destination=/$relative
    mkdir -p "$(dirname "$destination")" "$(dirname "$backup/$relative")"
    if test -e "$destination" || test -L "$destination"; then
        if ! test -e "$backup/$relative" && ! test -L "$backup/$relative"; then cp -P "$destination" "$backup/$relative"; fi
    fi
    ln -s "$source" "$destination.agx-web-new"
    mv -f "$destination.agx-web-new" "$destination"
done
ln -sfn "$release" /var/lib/agx-data/agx-web/current.new
mv -fT /var/lib/agx-data/agx-web/current.new /var/lib/agx-data/agx-web/current
rm "$archive"
echo "Installed ANGI Web runtime $release_id"'

if [[ "${AGX_WEB_RUNTIME_ONLY:-0}" == 1 ]]; then
    ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'rm -f /var/lib/agx-data/agx-web/gui.new'
    exit 0
fi
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'set -e
umask 022
old=$(readlink -f /usr/bin/agx-gui)
test -f "$old" || { echo "Install the base desktop first" >&2; exit 1; }
mkdir -p /var/lib/agx-data/agx-gui
backup=/var/lib/agx-data/agx-web/backup/agx-gui
if ! test -e "$backup"; then ln -s "$old" "$backup"; fi
staged=/var/lib/agx-data/agx-web/gui.new
id=$(sha256sum "$staged" | cut -c1-16)
new=/var/lib/agx-data/agx-gui/agx-gui.$id
if test -f "$new"; then rm "$staged"; else mv "$staged" "$new"; chmod 755 "$new"; fi
ln -s "$new" /usr/bin/agx-gui.agx-web-new
mv -fT /usr/bin/agx-gui.agx-web-new /usr/bin/agx-gui
if test "$new" != "$old"; then
    for pid in $(pidof agx-gui 2>/dev/null || true); do
        if test "$(readlink -f /proc/$pid/exe)" = "$old"; then kill -TERM "$pid"; fi
    done
fi
echo "Installed ANGI desktop $id. Sign in and open Web from Apps (Super+Space)."'
