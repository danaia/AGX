#!/usr/bin/env bash
# Install the Wayland runtime (wlroots, cage, foot and the libraries Chromium needs) on the NVMe of a running Pi.
#
#   scripts/deploy-wayland.sh            package the current runtime, install it, verify it loads
#   scripts/deploy-wayland.sh --remove   remove every link this script created, and the release files
#
# Files live under /var/lib/agx-data/agx-wayland/releases/ID; /usr gets symlinks to them. Nothing that already
# exists is ever replaced, /lib and /lib64 are never touched, and a manifest of every link makes removal exact.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
SSH_OPTIONS=(-o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=8 -o ConnectionAttempts=1
    -o ServerAliveInterval=5 -o ServerAliveCountMax=3 -o StrictHostKeyChecking=yes -i "$KEY")
if [[ -n "${AGX_SSH_HOST_KEY_ALIAS:-}" ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)
fi
SSH=(ssh "${SSH_OPTIONS[@]}" "$DESTINATION")
SCP=(scp -O "${SSH_OPTIONS[@]}")
BASE=/var/lib/agx-data/agx-wayland

if [[ ${1:-} == --remove ]]; then
    "${SSH[@]}" 'set -e
base='"$BASE"'
[ -f "$base/MANIFEST" ] || { echo "Nothing to remove: no Wayland runtime is installed."; exit 0; }
removed=0
while IFS= read -r path; do
    # Only ever remove a link that points into this runtime.
    case "$(readlink "$path" 2>/dev/null)" in "$base"/releases/*) rm -f "$path"; removed=$((removed + 1)) ;; esac
done < "$base/MANIFEST"
rm -rf "$base"
echo "Removed $removed links and the runtime files."'
    exit 0
fi

[[ -d "$ROOT/output-wayland/target" && -d "$ROOT/output/target" ]] ||
    { echo 'Build the ANGI image and run scripts/build-wayland.sh first.' >&2; exit 1; }
mkdir -p "$ROOT/.cache"
TAR=$(mktemp "$ROOT/.cache/wayland-runtime.XXXXXX")
trap 'rm -f "$TAR"' EXIT
python3 "$ROOT/scripts/package-wayland-runtime.py" \
    --target "$ROOT/output-wayland/target" --image "$ROOT/output/target" --output "$TAR"

"${SSH[@]}" 'mountpoint -q /var/lib/agx-data || { echo "The NVMe data volume must be mounted" >&2; exit 1; }
mkdir -p '"$BASE"
"${SCP[@]}" "$TAR" "$DESTINATION:$BASE/runtime.tar"
"${SSH[@]}" 'set -e
base='"$BASE"'
id=$(sha256sum "$base/runtime.tar" | cut -c1-16)
release=$base/releases/$id
mkdir -p "$release"
tar -xf "$base/runtime.tar" -C "$release"
: > "$base/MANIFEST.new"
find "$release" \( -type f -o -type l \) | sort | while IFS= read -r source; do
    relative=${source#"$release"/}
    case "$relative" in lib/*|lib64/*|etc/ld*) echo "Refusing to relocate a boot path: $relative" >&2; exit 1 ;; esac
    destination=/$relative
    # Never replace anything this runtime did not put there.
    if [ -e "$destination" ] || [ -L "$destination" ]; then
        case "$(readlink "$destination" 2>/dev/null)" in
            "$base"/releases/*) ;;
            *) echo "skipping $destination (already present)" >&2; continue ;;
        esac
    fi
    mkdir -p "$(dirname "$destination")"
    ln -sf "$source" "$destination.agx-new"
    mv -f "$destination.agx-new" "$destination"
    echo "$destination" >> "$base/MANIFEST.new"
done
# Links an earlier release made that this one no longer has.
if [ -f "$base/MANIFEST" ]; then
    while IFS= read -r path; do
        grep -qxF "$path" "$base/MANIFEST.new" || case "$(readlink "$path" 2>/dev/null)" in "$base"/releases/*) rm -f "$path" ;; esac
    done < "$base/MANIFEST"
fi
mv -f "$base/MANIFEST.new" "$base/MANIFEST"
ln -sfn "$release" "$base/current"
rm -f "$base/runtime.tar"
echo "Wayland runtime $id: $(wc -l < "$base/MANIFEST") links, $(du -sh "$release" | cut -f1) on the NVMe"'

echo 'Checking that the installed programs load every library they need'
"${SSH[@]}" 'bad=0
for program in /usr/bin/cage /usr/bin/foot /usr/bin/wayland-info /usr/bin/dbus-run-session /usr/bin/wayvnc /usr/bin/weston /usr/bin/seatd; do
    if ! dependencies=$(ldd "$program" 2>&1) || printf "%s\n" "$dependencies" | grep -q "not found"; then
        printf "%s: %s\n" "$program" "$dependencies" >&2
        bad=1
    else
        echo "$program: all libraries resolve"
    fi
done
[ -x /usr/bin/angi-chromium ] && [ -r /usr/libexec/agx-wayland/lib/libudev.so.1 ] || bad=1
exit $bad'
