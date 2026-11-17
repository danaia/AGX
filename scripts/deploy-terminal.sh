#!/usr/bin/env bash
# Stage CLI tools and GUI on NVMe, then atomically activate the tested binaries.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
SSH_OPTIONS=(-o BatchMode=yes -o ConnectTimeout=10 -i "$KEY")
if [[ -n "${AGX_SSH_HOST_KEY_ALIAS:-}" ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)
fi
BINARY=${AGX_GUI_BINARY:-"$ROOT/.cache/cargo-target/aarch64-unknown-linux-gnu/release/agx-gui"}
ARCHIVE="$ROOT/.cache/agx-terminal-runtime.tar"
python3 "$ROOT/scripts/package-terminal-runtime.py" "$ROOT/output/target" "$ARCHIVE"
"$ROOT/output/host/bin/aarch64-linux-strip" -o "$ROOT/.cache/agx-terminal-gui" "$BINARY"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'mountpoint -q /var/lib/agx-data && mkdir -p /var/lib/agx-data/agx-terminal'
scp -O "${SSH_OPTIONS[@]}" "$ARCHIVE" "$DESTINATION:/var/lib/agx-data/agx-terminal/runtime.tar"
scp -O "${SSH_OPTIONS[@]}" "$ROOT/.cache/agx-terminal-gui" "$DESTINATION:/var/lib/agx-data/agx-terminal/gui.new"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'set -e
umask 022
base=/var/lib/agx-data/agx-terminal
id=$(sha256sum "$base/runtime.tar" | cut -c1-16)
release=$base/releases/$id
if ! test -f "$release/.complete"; then
    stage=$release.new
    rm -rf "$stage"
    mkdir -p "$stage"
    tar -xf "$base/runtime.tar" -C "$stage"
    mkdir -p "$stage/usr/lib/locale"
    I18NPATH="$stage/usr/share/agx-terminal/i18n" "$stage/usr/libexec/agx-terminal-localedef" \
        --no-archive -f UTF-8 -i C "$stage/usr/lib/locale/C.utf8"
    touch "$stage/.complete"
    mv -T "$stage" "$release"
fi
find "$release" -type d | while IFS= read -r path; do
    chmod 755 "$path"
    if test "$path" != "$release"; then mkdir -p "/${path#"$release"/}"; fi
done
find "$release" \( -type f -o -type l \) | while IFS= read -r source; do
    relative=${source#"$release"/}
    case "$relative" in .complete) continue;; bin/bash|usr/*|etc/profile.d/agx-terminal.sh|etc/profile.d/agx-welcome.sh) ;; *) echo "Unexpected terminal path: $relative" >&2; exit 1;; esac
    destination=/$relative
    if { test -e "$destination" || test -L "$destination"; } && ! { test -e "$base/backup/$relative" || test -L "$base/backup/$relative"; }; then mkdir -p "$base/backup/${relative%/*}"; cp -P "$destination" "$base/backup/$relative"; fi
    ln -s "$source" "$destination.agx-terminal-new"
    mv -fT "$destination.agx-terminal-new" "$destination"
done
# Check tools before changing the running desktop. Never touch libc or its loader.
/bin/bash --version | head -1
LC_ALL=C.UTF-8 /usr/bin/tmux -V
/usr/bin/vim --version | head -1
/usr/bin/ssh -V
ln -sfn "$release" "$base/current.new"
mv -fT "$base/current.new" "$base/current"
rm "$base/runtime.tar"'
if [[ "${AGX_TERMINAL_RUNTIME_ONLY:-0}" == 1 ]]; then
    ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'rm -f /var/lib/agx-data/agx-terminal/gui.new'
    exit 0
fi
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'set -e
umask 022
base=/var/lib/agx-data/agx-terminal
old=$(readlink -f /usr/bin/agx-gui)
id=$(sha256sum "$base/gui.new" | cut -c1-16)
new=/var/lib/agx-data/agx-gui/agx-gui.$id
if test -f "$new"; then rm "$base/gui.new"; else mv "$base/gui.new" "$new"; chmod 755 "$new"; fi
if ! test -e "$base/previous-gui"; then ln -s "$old" "$base/previous-gui"; fi
ln -s "$new" /usr/bin/agx-gui.agx-terminal-new
mv -fT /usr/bin/agx-gui.agx-terminal-new /usr/bin/agx-gui
if test "$new" != "$old"; then
    for pid in $(pidof agx-gui 2>/dev/null || true); do
        if test "$(readlink -f /proc/$pid/exe)" = "$old"; then kill -TERM "$pid"; fi
    done
fi
echo "Installed ANGI Terminal $id. Sign in and open Terminal from Apps (Super+Space)."'
