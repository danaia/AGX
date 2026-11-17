#!/usr/bin/env bash
# Install Tailscale on the development Pi in userspace-networking mode (the kernel has no TUN).
#
# The pinned release is checked against its published SHA-256 before anything is copied. Binaries
# go to a versioned NVMe release and are linked into place atomically; the previous release stays
# for rollback. The tiny boot scripts are real files on the SD root so init finds them before the
# data volume mounts. Signing in to a tailnet is the account owner's step: this prints nothing
# secret and never uses an auth key (see docs/tailscale.md).
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=1.104.7
SHA256=d6bed2720310b328b861f943d7b4016c5358a9bad60575eb51e8143b79068484
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
OPERATOR=${AGX_TAILSCALE_OPERATOR:-dana}
OPTIONS=(-i "${AGX_SSH_KEY:-$HOME/.ssh/id_ed25519}" -o BatchMode=yes -o ConnectTimeout=10)
if [[ -n ${AGX_SSH_HOST_KEY_ALIAS:-} ]]; then OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes); fi

cache=$ROOT/.cache/tailscale
archive=$cache/tailscale_${VERSION}_arm64.tgz
mkdir -p "$cache"
if [[ ! -f $archive ]]; then
    curl -fL --retry 3 "https://pkgs.tailscale.com/stable/tailscale_${VERSION}_arm64.tgz" -o "$archive"
fi
printf '%s  %s\n' "$SHA256" "$archive" | sha256sum -c --status || { echo "Tailscale archive failed its checksum" >&2; exit 1; }

stage=$cache/stage
rm -rf "$stage" "$cache/unpacked"
mkdir -p "$stage/usr/bin" "$stage/usr/sbin" "$stage/usr/libexec/agx-tailscale" "$cache/unpacked"
tar -xzf "$archive" -C "$cache/unpacked"
install -m755 "$cache/unpacked/tailscale_${VERSION}_arm64/tailscale" "$stage/usr/bin/tailscale"
install -m755 "$cache/unpacked/tailscale_${VERSION}_arm64/tailscaled" "$stage/usr/sbin/tailscaled"
install -m755 "$ROOT/apps/tailscale/run.sh" "$stage/usr/libexec/agx-tailscale/run"
install -m755 "$ROOT/apps/tailscale/connect.sh" "$stage/usr/libexec/agx-tailscale/connect"
install -m644 "$ROOT/apps/tailscale/inetd.conf" "$stage/usr/libexec/agx-tailscale/inetd.conf"
tar -cf "$cache/runtime.tar" -C "$stage" .

ssh "${OPTIONS[@]}" "$DESTINATION" 'mountpoint -q /var/lib/agx-data && mkdir -p /var/lib/agx-data/tailscale'
scp -O "${OPTIONS[@]}" "$cache/runtime.tar" "$DESTINATION:/var/lib/agx-data/tailscale/runtime.tar"
ssh "${OPTIONS[@]}" "$DESTINATION" "OPERATOR=$OPERATOR /bin/sh -s" <<'INSTALL'
set -eu
base=/var/lib/agx-data/tailscale
identifier=$(sha256sum "$base/runtime.tar" | cut -c1-16)
release=$base/releases/$identifier
mkdir -p "$release"
tar -xf "$base/runtime.tar" -C "$release"
chown -R 0:0 "$release"
chmod -R go-w "$release"
echo "previous release: $(readlink "$base/current" 2>/dev/null || echo none)"
# Binaries live on NVMe and are linked in; the boot scripts are regular files on the SD root.
for path in usr/bin/tailscale usr/sbin/tailscaled; do
    ln -s "$release/$path" "/$path.new"
    mv -f "/$path.new" "/$path"
done
mkdir -p /usr/libexec/agx-tailscale
for path in run connect inetd.conf; do
    cp "$release/usr/libexec/agx-tailscale/$path" "/usr/libexec/agx-tailscale/$path.new"
    mv -f "/usr/libexec/agx-tailscale/$path.new" "/usr/libexec/agx-tailscale/$path"
done
ln -sfn "$release" "$base/current"
rm "$base/runtime.tar"
tailscale version | head -1

# Until agx-init supervises it (after the next reboot), start it directly, once.
if ! pidof tailscaled >/dev/null; then
    mkdir -p /run/tailscale
    setsid nohup /bin/sh /usr/libexec/agx-tailscale/run </dev/null >/run/tailscale/run.log 2>&1 &
fi
for _ in 1 2 3 4 5 6 7 8 9 10; do [ -S /run/tailscale/tailscaled.sock ] && break; sleep 1; done
# Safe preferences, set once as root and kept in the daemon's state: nothing can connect in,
# the Pi's DNS and routes are never changed, and the desktop user may sign in without root.
tailscale --socket=/run/tailscale/tailscaled.sock set --shields-up=true --accept-dns=false --accept-routes=false \
    --hostname=agx --operator="$OPERATOR"
tailscale --socket=/run/tailscale/tailscaled.sock status 2>&1 | head -3 || true
INSTALL
