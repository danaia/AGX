#!/bin/bash
set -euo pipefail
source /usr/share/agx/agent/sdk-env.sh
umask 077
build_store=${CARGO_HOME%/cargo}
mkdir -m700 -- "$build_store" 2>/dev/null || true
[[ -d $build_store && -O $build_store && ! -L $build_store ]] || { echo 'Unsafe ANGI build storage.' >&2; exit 1; }
chmod 700 "$build_store"
cd /var/lib/agx-data/angi-dev
case "${1:-}" in
    cli) cargo build --locked --release -p agx-cli ;;
    gui) cargo build --locked --release --manifest-path crates/agx-gui/Cargo.toml --no-default-features --features kms ;;
    test) cargo test --locked --workspace -- --test-threads=1 ;;
    gui-test) cargo test --locked --manifest-path crates/agx-gui/Cargo.toml --no-default-features --features kms ;;
    env) echo /usr/share/agx/agent/sdk-env.sh ;;
    *) echo 'Usage: angi-build {cli|gui|test|gui-test|env}' >&2; exit 2 ;;
esac
