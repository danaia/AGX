#!/usr/bin/env bash
# Link the ANGI shell adapter with the Weston ABI in the Wayland sysroot.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
OUTPUT="$ROOT/output-wayland"
SYSROOT="$OUTPUT/staging"
export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"
export PKG_CONFIG_LIBDIR="$SYSROOT/usr/lib/pkgconfig"
read -r -a flags <<<"$("$OUTPUT/host/bin/pkgconf" --cflags --libs libweston-15 wayland-server)"
mkdir -p "$OUTPUT/target/usr/lib/weston"
"$OUTPUT/host/bin/aarch64-linux-gcc" -std=gnu11 -O2 -Wall -Wextra -Werror -shared -fPIC \
    -Wl,-z,defs "$ROOT/apps/wayland/angi-shell.c" "${flags[@]}" \
    -o "$OUTPUT/target/usr/lib/weston/angi-shell.so"
