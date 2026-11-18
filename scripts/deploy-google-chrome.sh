#!/usr/bin/env bash
# Verify Google's stable ARM64 package and install it on the ANGI development Pi.
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
base_url=https://dl.google.com/linux/chrome/deb
index=main/binary-arm64/Packages.gz
fingerprint=EB4C1BFD4F042F6DDDCCEC917721F63BD38B4796
base=/var/lib/agx-data/agx-google-chrome
: "${AGX_SSH_HOST:?Set AGX_SSH_HOST from docs/pi-ethernet.md}"
: "${AGX_SSH_KEY:?Set AGX_SSH_KEY from docs/pi-ethernet.md}"
: "${AGX_SSH_HOST_KEY_ALIAS:?Set AGX_SSH_HOST_KEY_ALIAS from docs/pi-ethernet.md}"
work=$(mktemp -d)
remote_stage=
ssh_options=(-i "$AGX_SSH_KEY" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=8
             -o ConnectionAttempts=1 -o ServerAliveInterval=5 -o ServerAliveCountMax=3
             -o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)

pi() { ssh "${ssh_options[@]}" "$AGX_SSH_HOST" "$@"; }
download() { curl --proto '=https' --location --fail --silent --show-error "$1" -o "$2"; }
check_hash() { printf '%s  %s\n' "$1" "$2" | sha256sum -c -; }
cleanup() {
    if [[ -n $remote_stage ]]; then
        pi "rm -rf -- '$remote_stage'" || true
    fi
    rm -rf -- "$work"
}
trap cleanup EXIT

download https://dl.google.com/linux/linux_signing_key.pub "$work/key.asc"
gpg --batch --no-options --homedir "$work" --dearmor --output "$work/google.gpg" "$work/key.asc"
download "$base_url/dists/stable/InRelease" "$work/InRelease"
gpgv --keyring "$work/google.gpg" --status-fd 1 --output "$work/Release" "$work/InRelease" >"$work/status"
awk -v fingerprint="$fingerprint" '$2 == "VALIDSIG" && $NF == fingerprint { valid = 1 } END { exit !valid }' "$work/status"

index_hash=$(awk -v path="$index" '
    /^SHA256:$/ { in_section = 1; next }
    /^[^ ]/ { in_section = 0 }
    in_section && $3 == path { print $1 }
' "$work/Release")
download "$base_url/dists/stable/$index" "$work/Packages.gz"
check_hash "$index_hash" "$work/Packages.gz"
gzip -dc "$work/Packages.gz" >"$work/Packages"
package=$(awk -v RS='' -F '\n' '
    {
        name = arch = version = file = hash = ""
        for (i = 1; i <= NF; i++) {
            line = $i
            if (sub(/^Package: /, "", line)) name = line
            else if (sub(/^Architecture: /, "", line)) arch = line
            else if (sub(/^Version: /, "", line)) version = line
            else if (sub(/^Filename: /, "", line)) file = line
            else if (sub(/^SHA256: /, "", line)) hash = line
        }
        if (name == "google-chrome-stable" && arch == "arm64") print version, file, hash
    }
' "$work/Packages")
read -r version file digest <<<"$package"
if [[ $package == *$'\n'* || ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+-[0-9]+$ ||
      $file != "pool/main/g/google-chrome-stable/google-chrome-stable_${version}_arm64.deb" ||
      ! $digest =~ ^[0-9a-f]{64}$ ]]; then
    echo "Expected one Google Chrome stable ARM64 package in Google's signed index." >&2
    exit 1
fi
download "$base_url/$file" "$work/chrome.deb"
check_hash "$digest" "$work/chrome.deb"
ar p "$work/chrome.deb" data.tar.xz >"$work/data.tar.xz"
payload=$(sha256sum "$work/data.tar.xz")
payload=${payload%% *}
launcher="$repo/apps/wayland/google-chrome.sh"
launcher_hash=$(sha256sum "$launcher")
launcher_hash=${launcher_hash%% *}
release="$version-${digest:0:12}-${launcher_hash:0:12}"

pi 'test "$(id -u)" -eq 0 && test "$(uname -m)" = aarch64 && mountpoint -q /var/lib/agx-data && test -r /usr/libexec/agx-wayland/lib/libudev.so.1'
stage=$(pi "mkdir -p '$base/releases' && mktemp -d '$base/.deploy.XXXXXXXX'")
[[ $stage =~ ^/var/lib/agx-data/agx-google-chrome/\.deploy\.[a-zA-Z0-9]+$ ]]
remote_stage=$stage
scp -O "${ssh_options[@]}" "$work/data.tar.xz" "$launcher" "$AGX_SSH_HOST:$remote_stage/"
pi sh -s -- "$base" "$remote_stage" "$release" "$payload" "$launcher_hash" <<'PI'
set -eu
base=$1 stage=$2 release=$3 payload=$4 launcher_hash=$5
cd "$stage"
printf '%s  data.tar.xz\n%s  google-chrome.sh\n' "$payload" "$launcher_hash" | sha256sum -c -

launcher=/usr/bin/google-chrome
target=$base/current/angi-launch
if [ -e "$launcher" ] || [ -L "$launcher" ]; then
    test "$(readlink "$launcher")" = "$target"
fi
destination=$base/releases/$release
if [ ! -d "$destination" ]; then
    mkdir extract
    xz -dc data.tar.xz | tar -xf - -C extract ./opt/google/chrome
    chrome=extract/opt/google/chrome
    test -x "$chrome/chrome"
    cp google-chrome.sh "$chrome/angi-launch"
    chmod 755 "$chrome/angi-launch"
    chown -R 0:0 "$chrome"
    chmod -R a-s "$chrome"
    "$chrome/angi-launch" --version
    if ! mv -T "$chrome" "$destination"; then
        test -x "$destination/angi-launch"
    fi
fi

ln -s "releases/$release" next
mv -fT next "$base/current"
if [ ! -L "$launcher" ]; then
    ln -s "$target" "$launcher"
fi
"$launcher" --version
PI
