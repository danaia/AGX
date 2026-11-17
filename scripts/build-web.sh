#!/usr/bin/env bash
# Cross-build the pinned WPEPlatform engine and its small ANGI embedding host.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$ROOT/.cache/host-tools/usr/bin:$PATH"
"$ROOT/scripts/build.sh" prepare
SOURCE="$ROOT/.cache/src/buildroot-2026.08.1"
"$SOURCE/utils/config" --file "$ROOT/output/.config" --enable BR2_PACKAGE_AGX_WEB
make -C "$SOURCE" O="$ROOT/output" BR2_EXTERNAL="$ROOT/buildroot" olddefconfig
make -C "$SOURCE" O="$ROOT/output" BR2_EXTERNAL="$ROOT/buildroot" \
    "BR2_JLEVEL=${WEBKIT_BUILD_JOBS:-3}" agx-web-rebuild
mkdir -p "$ROOT/.cache/agx-web"
"$ROOT/output/host/bin/pkg-config" --atleast-version=2.54 wpe-webkit-2.0
"$ROOT/output/host/bin/aarch64-linux-strip" -o "$ROOT/.cache/agx-web/agx-web" "$ROOT/output/target/usr/libexec/agx-web"
