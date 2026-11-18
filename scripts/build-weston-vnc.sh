#!/usr/bin/env bash
# Enable Weston's DRM mirror for the appliance's loopback OpenSSH VNC tunnel.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
OUTPUT="$ROOT/output-wayland"
CACHE="$ROOT/.cache/weston-vnc-src"
export PATH="$OUTPUT/host/bin:$PATH"
mkdir -p "$CACHE"

checkout() {
    local name=$1 tag=$2 revision=$3
    if [[ ! -d "$CACHE/$name/.git" ]]; then
        git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$tag" "https://github.com/any1/$name.git" "$CACHE/$name"
    fi
    [[ $(git -C "$CACHE/$name" rev-parse HEAD) == "$revision" ]] ||
        { echo "Unexpected revision for $name" >&2; return 1; }
}
checkout aml v0.3.0 b83f3576ce4187d9285f06e9066ef43a691464d4
checkout neatvnc v0.9.6 3295c11b934a83dbcb44beabf3f21a8b885a2d11

weston="$OUTPUT/build/weston-15.0.1"
build="$weston/buildroot-build"
mkdir -p "$weston/subprojects"
ln -sfnT "$CACHE/aml" "$weston/subprojects/aml"
ln -sfnT "$CACHE/neatvnc" "$weston/subprojects/neatvnc"
vnc_patch="$ROOT/apps/wayland/patches/weston-loopback-vnc.patch"
if ! patch --silent --dry-run -R -d "$weston" -p1 < "$vnc_patch" >/dev/null 2>&1; then
    patch -d "$weston" -p1 < "$vnc_patch"
fi
meson setup --reconfigure "$build" "$weston" -Dbackend-vnc=true
meson configure "$build" -Dneatvnc:default_library=static -Daml:default_library=static \
    -Dneatvnc:tls=disabled -Dneatvnc:nettle=disabled -Dneatvnc:h264=disabled
meson compile -C "$build"
DESTDIR="$OUTPUT/target" meson install -C "$build" --no-rebuild --strip
