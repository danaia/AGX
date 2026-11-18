#!/usr/bin/env bash
# Build the Wayland user-space runtime (wlroots, cage, Chromium's support libraries) into output-wayland/,
# separate from the image build in output/. Usage: build-wayland.sh [configure|build]
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# Workstation tools Buildroot needs (bc, wget) that are not installed system-wide; same as the other build scripts.
export PATH="$ROOT/.cache/host-tools/usr/bin:$PATH"
SOURCE="$ROOT/.cache/src/buildroot-2026.08.1"
OUTPUT="$ROOT/output-wayland"
[[ -d "$SOURCE" ]] || { echo 'Run scripts/build.sh prepare first to fetch Buildroot.' >&2; exit 1; }
brmake() { make -C "$SOURCE" O="$OUTPUT" BR2_EXTERNAL="$ROOT/buildroot" "$@"; }
brmake agx_wayland_defconfig
brmake olddefconfig
[[ ${1:-build} == configure ]] && exit 0
brmake "BR2_JLEVEL=${JOBS:-12}"
"$ROOT/scripts/build-wayland-viewer.sh"
"$ROOT/scripts/build-weston-vnc.sh"
"$ROOT/scripts/build-native-wayland-shell.sh"
