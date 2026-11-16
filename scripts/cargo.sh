#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
if [[ -x "$ROOT/.cache/cargo/bin/cargo" ]]; then
    export RUSTUP_HOME="$ROOT/.cache/rustup"
    export CARGO_HOME="$ROOT/.cache/cargo"
    export PATH="$CARGO_HOME/bin:$PATH"
elif [[ -x "$ROOT/output/host/bin/cargo" ]]; then
    export CARGO_HOME="${CARGO_HOME:-$ROOT/.cache/cargo}"
    export PATH="$ROOT/output/host/bin:$PATH"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.cache/cargo-target}"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:-$ROOT/output/host/bin/aarch64-linux-gcc}"
exec cargo "$@"
