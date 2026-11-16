#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
STAGE="$ROOT/.cache/agx-package"
mkdir -p "$STAGE/.cargo"
rsync -a --delete "$ROOT/crates/" "$STAGE/crates/"
install -m 644 "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$ROOT/rust-toolchain.toml" "$STAGE/"
"$ROOT/scripts/cargo.sh" vendor --locked --versioned-dirs "$STAGE/vendor" > "$STAGE/.cargo/config.toml.tmp"
python3 - "$STAGE" <<'PY'
from pathlib import Path
import sys
stage = Path(sys.argv[1])
config = (stage / '.cargo/config.toml.tmp').read_text().replace(str(stage / 'vendor'), 'vendor')
(stage / '.cargo/config.toml').write_text(config)
(stage / '.cargo/config.toml.tmp').unlink()
PY
