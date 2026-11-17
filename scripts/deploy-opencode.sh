#!/usr/bin/env bash
# Install OpenCode on NVMe without changing boot libraries or user credentials.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
SSH_OPTIONS=(-o BatchMode=yes -o ConnectTimeout=10 -i "$KEY")
if [[ -n "${AGX_SSH_HOST_KEY_ALIAS:-}" ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)
fi
ARCHIVE="$ROOT/.cache/agx-opencode-runtime.tar"
python3 "$ROOT/scripts/package-opencode-runtime.py" "$ROOT/output/target" "$ARCHIVE"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'mountpoint -q /var/lib/agx-data && test -x /bin/bash && mkdir -p /var/lib/agx-data/agx-opencode'
scp -O "${SSH_OPTIONS[@]}" "$ARCHIVE" "$DESTINATION:/var/lib/agx-data/agx-opencode/runtime.tar"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'set -e
umask 022
base=/var/lib/agx-data/agx-opencode
id=$(sha256sum "$base/runtime.tar" | cut -c1-16)
release=$base/releases/$id
if ! test -f "$release/.complete"; then
    stage=$release.new
    rm -rf "$stage"
    mkdir -p "$stage"
    tar -xf "$base/runtime.tar" -C "$stage"
    "$stage/usr/libexec/opencode/opencode" --version
    touch "$stage/.complete"
    mv -T "$stage" "$release"
fi
find "$release" -type d | while IFS= read -r path; do
    chmod 755 "$path"
    if test "$path" != "$release"; then mkdir -p "/${path#"$release"/}"; fi
done
find "$release" \( -type f -o -type l \) | while IFS= read -r source; do
    relative=${source#"$release"/}
    case "$relative" in .complete) continue;; usr/*|etc/init.d/S12agx-data) ;; *) echo "Unexpected OpenCode path: $relative" >&2; exit 1;; esac
    destination=/$relative
    if { test -e "$destination" || test -L "$destination"; } && ! { test -e "$base/backup/$relative" || test -L "$base/backup/$relative"; }; then
        mkdir -p "$base/backup/${relative%/*}"
        cp -P "$destination" "$base/backup/$relative"
    fi
    ln -s "$source" "$destination.agx-opencode-new"
    mv -fT "$destination.agx-opencode-new" "$destination"
done
users=/var/lib/agx-data/agx-opencode-users
test ! -L "$users"
mkdir -p "$users"
chown 0:0 "$users"
chmod 1777 "$users"
ln -sfn "$release" "$base/current.new"
mv -fT "$base/current.new" "$base/current"
rm "$base/runtime.tar"
git --version
rg --version | head -1
echo "Installed ANGI OpenCode runtime $id."'
