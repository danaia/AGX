#!/usr/bin/env bash
# Build Git/ripgrep and install the checksum-pinned official OpenCode ARM64 CLI.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$ROOT/.cache/host-tools/usr/bin:$PATH"
"$ROOT/scripts/build.sh" prepare
SOURCE="$ROOT/.cache/src/buildroot-2026.08.1"
"$SOURCE/utils/config" --file "$ROOT/output/.config" --enable BR2_PACKAGE_AGX_OPENCODE
"$SOURCE/utils/config" --file "$ROOT/output/.config" --set-str BR2_STRIP_EXCLUDE_FILES opencode
make -C "$SOURCE" O="$ROOT/output" BR2_EXTERNAL="$ROOT/buildroot" olddefconfig
make -C "$SOURCE" O="$ROOT/output" BR2_EXTERNAL="$ROOT/buildroot" "BR2_JLEVEL=${OPENCODE_BUILD_JOBS:-4}" agx-opencode
