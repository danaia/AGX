#!/usr/bin/env bash
# Cross-build the local viewer against the existing Wayland Buildroot sysroot.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
OUTPUT="$ROOT/output-wayland"
SOURCE="$ROOT/.cache/wayvnc-src"
export PATH="$OUTPUT/host/bin:$PATH"
mkdir -p "$SOURCE"

checkout() {
    local name=$1 repository=$2 tag=$3 revision=$4 directory="$SOURCE/$1"
    if [[ ! -d "$directory/.git" ]]; then
        git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$tag" "$repository" "$directory"
    fi
    [[ $(git -C "$directory" rev-parse HEAD) == "$revision" ]] ||
        { echo "Unexpected source revision in $directory" >&2; return 1; }
}

checkout wayvnc https://github.com/any1/wayvnc.git v0.10.2 bb837459c75fd3c267d48af170ac7ed2566f67e5
checkout neatvnc https://github.com/any1/neatvnc.git v1.0.4 ffce32d6d04e702d5d8055e28e038c7f68a0caf4
checkout aml https://github.com/any1/aml.git v1.0.0 685035c9830aa89df02a43df89b644690bd885f5

mkdir -p "$SOURCE/wayvnc/subprojects"
ln -sfnT "$SOURCE/aml" "$SOURCE/wayvnc/subprojects/aml"
ln -sfnT "$SOURCE/neatvnc" "$SOURCE/wayvnc/subprojects/neatvnc"
meson setup --reconfigure "$OUTPUT/wayvnc-build" "$SOURCE/wayvnc" \
    --cross-file "$OUTPUT/host/etc/meson/cross-compilation.conf" \
    --prefix=/usr --buildtype=release --default-library=static \
    -Dpam=disabled -Dman-pages=disabled -Dtests=false \
    -Dneatvnc:tls=disabled -Dneatvnc:nettle=disabled -Dneatvnc:h264=disabled
meson compile -C "$OUTPUT/wayvnc-build"
install -m755 "$OUTPUT/wayvnc-build/wayvnc" "$OUTPUT/target/usr/bin/wayvnc"
aarch64-linux-strip "$OUTPUT/target/usr/bin/wayvnc"
