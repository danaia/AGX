#!/usr/bin/env bash
# Provision a self-contained native development workspace without overwriting it.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
DEV_USER=${AGX_DEV_USER:-dana}
[[ $DEV_USER =~ ^[a-z_][a-z0-9_-]*$ ]] || exit 2
OPTIONS=(-i "${AGX_SSH_KEY:-$HOME/.ssh/id_ed25519}" -o BatchMode=yes -o ConnectTimeout=10)
if [[ -n ${AGX_SSH_HOST_KEY_ALIAS:-} ]]; then OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS"); fi
python3 "$ROOT/scripts/prepare-agent-sdk.py"
cache=$ROOT/.cache/angi-dev
if [[ ! -f $cache/rustup-init ]]; then
    curl -fL --retry 3 https://static.rust-lang.org/rustup/archive/1.29.3/aarch64-unknown-linux-gnu/rustup-init -o "$cache/rustup-init"
fi
printf '%s  %s\n' 9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282 "$cache/rustup-init" | sha256sum -c --status
tar -cf "$cache/sdk.tar" -C "$cache/sdk" .
tar -cf "$cache/sysroot.tar" -C "$ROOT/output/staging" .
(cd "$ROOT"; git ls-files -co --exclude-standard -z | tar --null -T - -cf "$cache/source.tar")
ssh "${OPTIONS[@]}" "$DESTINATION" 'mountpoint -q /var/lib/agx-data && mkdir -p /var/lib/agx-data/angi-sdk'
scp -O "${OPTIONS[@]}" "$cache/sdk.tar" "$cache/sysroot.tar" "$cache/source.tar" "$cache/rustup-init" "$ROOT/scripts/install-agent-sdk.sh" "$DESTINATION:/var/lib/agx-data/angi-sdk/"
ssh "${OPTIONS[@]}" "$DESTINATION" "/bin/bash -s -- $DEV_USER" <<'INSTALL'
set -euo pipefail
account=$1
uid=$(id -u "$account")
gid=$(id -g "$account")
base=/var/lib/agx-data/angi-sdk
/bin/bash "$base/install-agent-sdk.sh"
store=/var/lib/agx-data/agx-opencode-users/$uid
test ! -L "$store"
mkdir -p "$store"
chown "$uid:$gid" "$store"
chmod 700 "$store"
workspace=/var/lib/agx-data/angi-dev
if test -e "$workspace"; then
    echo "Preserving existing ANGI source workspace: $workspace"
else
    mkdir "$workspace"
    tar -xf "$base/source.tar" -C "$workspace"
    chown -R "$uid:$gid" "$workspace"
    cd "$workspace"
    su "$account" -c 'git init -q && git add . && git -c user.name=ANGI -c user.email=local@angi.invalid commit -qm Initial-native-workspace'
fi
rm "$base/sdk.tar" "$base/sysroot.tar" "$base/source.tar"
INSTALL
echo 'Native SDK and source provisioned. Run make deploy-agent next.'
