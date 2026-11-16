#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# Optional signed distro tools extracted locally on the development workstation.
if [[ -d "$ROOT/.cache/host-tools/usr/bin" ]]; then
    export PATH="$ROOT/.cache/host-tools/usr/bin:$PATH"
fi
VERSION=2026.08.1
SHA256=34fb7bea8a213ffde39cc02e61703391560f26e0850f03138c722d0b09880f2b
ARCHIVE="$ROOT/.cache/downloads/buildroot-$VERSION.tar.xz"
SOURCE="$ROOT/.cache/src/buildroot-$VERSION"
OUTPUT="$ROOT/output"
ACTION=${1:-build}
case "$ACTION" in
    prepare|configure|build|dependencies) ;;
    *) echo "Usage: $0 {prepare|configure|dependencies|build}" >&2; exit 2 ;;
esac
mkdir -p "$(dirname "$ARCHIVE")" "$(dirname "$SOURCE")"
if [[ ! -f "$ARCHIVE" ]]; then
    curl -fL --retry 3 "https://buildroot.org/downloads/buildroot-$VERSION.tar.xz" -o "$ARCHIVE.part"
    mv "$ARCHIVE.part" "$ARCHIVE"
fi
printf '%s  %s\n' "$SHA256" "$ARCHIVE" | sha256sum --check --status
if [[ ! -d "$SOURCE" ]]; then
    tar -xJf "$ARCHIVE" -C "$(dirname "$SOURCE")"
fi
[[ "$ACTION" == prepare ]] && exit 0
# Keep workstation-specific public keys out of the tracked board overlay.
PUBLIC_KEY=${SSH_PUBLIC_KEY:-"${HOME}/.ssh/id_ed25519.pub"}
if [[ ! -f "$PUBLIC_KEY" ]]; then
    echo "Set SSH_PUBLIC_KEY to an SSH public key file before configuring/building." >&2
    exit 1
fi
ssh-keygen -lf "$PUBLIC_KEY" >/dev/null
if ! grep -qE '^(ssh-ed25519|ssh-rsa|ecdsa-sha2-)[^ ]* ' "$PUBLIC_KEY"; then
    echo "Expected an OpenSSH public key, not a private key or certificate." >&2
    exit 1
fi
install -d -m 700 "$ROOT/.cache/local-overlay/root/.ssh"
install -m 600 "$PUBLIC_KEY" "$ROOT/.cache/local-overlay/root/.ssh/authorized_keys"
brmake() {
    make -C "$SOURCE" O="$OUTPUT" BR2_EXTERNAL="$ROOT/buildroot" "$@"
}
if [[ "$ACTION" == configure || ! -f "$OUTPUT/.config" ]]; then
    brmake agx_rpi5_defconfig
fi
case "$ACTION" in
    configure) ;;
    dependencies) brmake dependencies ;;
    build)
        if grep -q '^BR2_PACKAGE_AGX=y$' "$OUTPUT/.config"; then
            # Supply Cargo for vendoring even on hosts without a Rust installation.
            brmake "BR2_JLEVEL=${JOBS:-8}" host-rustc
            "$ROOT/scripts/prepare-rust-package.sh"
            # Local source packages need an explicit rebuild to pick up edits.
            brmake "BR2_JLEVEL=${JOBS:-8}" agx-rebuild
        fi
        # Buildroot does not otherwise recopy changed external firmware configuration.
        if ! cmp -s "$ROOT/buildroot/board/agx/config.txt" "$OUTPUT/images/rpi-firmware/config.txt" ||
           ! cmp -s "$ROOT/buildroot/board/agx/cmdline.txt" "$OUTPUT/images/rpi-firmware/cmdline.txt"; then
            brmake "BR2_JLEVEL=${JOBS:-8}" rpi-firmware-rebuild
        fi
        brmake "BR2_JLEVEL=${JOBS:-8}" all
        ;;
esac
