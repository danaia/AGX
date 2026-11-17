#!/usr/bin/env bash
# Build established CLI tools against ANGI's pinned Buildroot toolchain.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$ROOT/.cache/host-tools/usr/bin:$PATH"
"$ROOT/scripts/build.sh" prepare
SOURCE="$ROOT/.cache/src/buildroot-2026.08.1"
CONFIG="$SOURCE/utils/config"
for package in BASH TMUX NCURSES_WCHAR NCURSES_TARGET_PROGS LESS VIM VIM_RUNTIME OPENSSH OPENSSH_CLIENT OPENSSH_KEY_UTILS; do
    "$CONFIG" --file "$ROOT/output/.config" --enable "BR2_PACKAGE_$package"
done
"$CONFIG" --file "$ROOT/output/.config" --disable BR2_PACKAGE_OPENSSH_SERVER
"$CONFIG" --file "$ROOT/output/.config" --set-str BR2_PACKAGE_NCURSES_ADDITIONAL_TERMINFO 'x/xterm-256color t/tmux-256color'
"$CONFIG" --file "$ROOT/output/.config" --set-str BR2_GENERATE_LOCALE 'C.UTF-8'
"$CONFIG" --file "$ROOT/output/.config" --set-str BR2_ENABLE_LOCALE_WHITELIST 'C C.utf8 en_US'
make -C "$SOURCE" O="$ROOT/output" BR2_EXTERNAL="$ROOT/buildroot" olddefconfig
make -C "$SOURCE" O="$ROOT/output" BR2_EXTERNAL="$ROOT/buildroot" "BR2_JLEVEL=${TERMINAL_BUILD_JOBS:-4}" bash tmux less vim openssh
