#!/usr/bin/env bash
# Install the browser distribution and its license alongside the work-review page.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
destination=$1
version=0.52.2
archive=${2:-$ROOT/.cache/monaco/monaco-editor-$version.tgz}
digest=c280cdcf0b0c13d1a2bf01af958d4387ed06d7f6c918401d00c4adcae1bc72b6
if [[ ! -f $archive && $# == 1 ]]; then
    mkdir -p "$(dirname "$archive")"
    curl -fL --retry 3 "https://registry.npmjs.org/monaco-editor/-/monaco-editor-$version.tgz" -o "$archive.part"
    mv "$archive.part" "$archive"
fi
printf '%s  %s\n' "$digest" "$archive" | sha256sum -c --status
mkdir -p "$destination"
tar -xzf "$archive" -C "$destination" --strip-components=2 package/min/vs
tar -xzf "$archive" -C "$destination" --strip-components=1 package/LICENSE package/ThirdPartyNotices.txt
install -m644 "$ROOT/apps/agent/work-editor/"* "$destination/"
