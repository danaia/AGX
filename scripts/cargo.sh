#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
OUTPUT=${AGX_BUILD_OUTPUT:-"$ROOT/output"}
if [[ -x "$ROOT/.cache/cargo/bin/cargo" ]]; then
    export RUSTUP_HOME="$ROOT/.cache/rustup"
    export CARGO_HOME="$ROOT/.cache/cargo"
    export PATH="$CARGO_HOME/bin:$PATH"
elif [[ -x "$OUTPUT/host/bin/cargo" ]]; then
    export CARGO_HOME="${CARGO_HOME:-$ROOT/.cache/cargo}"
    export PATH="$OUTPUT/host/bin:$PATH"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:-$OUTPUT/host/bin/aarch64-linux-gcc}"
exec cargo "$@"
