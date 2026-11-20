#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

vendor() {
    local stage=$1
    shift
    mkdir -p "$stage/.cargo"
    "$ROOT/scripts/cargo.sh" vendor --locked --versioned-dirs "$@" "$stage/vendor" > "$stage/.cargo/config.toml.tmp"
    python3 - "$stage" <<'PYTHON'
from pathlib import Path
import sys
stage = Path(sys.argv[1])
temporary = stage / '.cargo/config.toml.tmp'
(stage / '.cargo/config.toml').write_text(temporary.read_text().replace(str(stage / 'vendor'), 'vendor'))
temporary.unlink()
PYTHON
}

STAGE="$ROOT/.cache/agx-package"
mkdir -p "$STAGE"
rsync -a --delete "$ROOT/crates/" "$STAGE/crates/"
install -m 644 "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$ROOT/rust-toolchain.toml" "$STAGE/"
vendor "$STAGE"

if [[ ${1:-pi5} == zero2w ]]; then
    GUI_STAGE="$ROOT/.cache/agx-gui-package"
    mkdir -p "$GUI_STAGE"
    rsync -a --delete "$ROOT/crates/" "$GUI_STAGE/crates/"
    rsync -a --delete "$ROOT/brand/" "$GUI_STAGE/brand/"
    install -D -m 644 "$ROOT/apps/agent/apps.json" "$GUI_STAGE/apps/agent/apps.json"
    install -m 644 "$ROOT/Cargo.toml" "$GUI_STAGE/Cargo.toml"
    vendor "$GUI_STAGE" --manifest-path "$GUI_STAGE/crates/agx-gui/Cargo.toml"
fi
