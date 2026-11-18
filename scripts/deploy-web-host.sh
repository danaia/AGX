#!/usr/bin/env bash
# Rebuild and install only the small Web embedding host (apps/web/*.c), not the WebKit runtime or the desktop.
# The host is what loads pages, so a change such as opening local HTML files needs only this. The engine
# runtime and the GUI release ledger are left alone. The previous host stays on NVMe for rollback.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
DESTINATION=${AGX_SSH_HOST:-root@10.77.0.2}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
SSH_OPTIONS=(-o BatchMode=yes -o ConnectTimeout=10 -i "$KEY")
if [[ -n "${AGX_SSH_HOST_KEY_ALIAS:-}" ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)
fi
TOOLS="$ROOT/output/host/bin"
OUT="$ROOT/.cache/agx-web-host"
[[ -x "$TOOLS/aarch64-linux-gcc" && -x "$TOOLS/pkg-config" ]] || { echo 'Build the image toolchain first (scripts/build.sh).' >&2; exit 1; }
mkdir -p "$OUT"
# The same sources, flags and libraries as buildroot/package/agx-web/agx-web.mk.
(cd "$ROOT/apps/web" && PKG_CONFIG_ALLOW_CROSS=1 "$TOOLS/aarch64-linux-gcc" -Os -std=gnu11 -DAGX_WEB_APPLIANCE=1 -Wall -Wextra -Werror \
    host.c tabs.c platform.c protocol.c dialogs.c storage.c gpu.c \
    $("$TOOLS/pkg-config" --cflags --libs wpe-webkit-2.0 libdrm) -o "$OUT/agx-web")
"$TOOLS/aarch64-linux-strip" -o "$OUT/agx-web.stripped" "$OUT/agx-web"
HASH=$(sha256sum "$OUT/agx-web.stripped" | cut -c1-16)
scp -O "${SSH_OPTIONS[@]}" "$OUT/agx-web.stripped" "$DESTINATION:/var/lib/agx-data/agx-web/agx-web-host.new"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" "set -e
mountpoint -q /var/lib/agx-data
host=/var/lib/agx-data/agx-web/host
mkdir -p \$host
chmod 755 /var/lib/agx-data/agx-web/agx-web-host.new
new=\$host/agx-web.$HASH
mv -f /var/lib/agx-data/agx-web/agx-web-host.new \"\$new\"
previous=\$(readlink -f /usr/libexec/agx-web)
if [ \"\$previous\" != \"\$new\" ]; then echo \"\$previous\" > \$host/previous; fi
ln -sf \"\$new\" /usr/libexec/agx-web.new
mv -f /usr/libexec/agx-web.new /usr/libexec/agx-web
echo \"Web host: \$(readlink -f /usr/libexec/agx-web) (previous: \$(cat \$host/previous))\""
echo 'Close and reopen Web to start the new host. To roll back: ln -sf "$(cat /var/lib/agx-data/agx-web/host/previous)" /usr/libexec/agx-web.new && mv -f /usr/libexec/agx-web.new /usr/libexec/agx-web'
