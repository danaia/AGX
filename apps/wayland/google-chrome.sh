#!/bin/sh
set -eu

install_dir=$(dirname "$(readlink -f "$0")")
export LD_LIBRARY_PATH="/usr/libexec/agx-wayland/lib:/usr/libexec/agx-wayland/egl${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

if [ "$#" -eq 1 ] && [ "$1" = --version ]; then
    exec "$install_dir/chrome" "$@"
fi

if [ "$(id -u)" -eq 0 ]; then
    echo "Start Google Chrome as a signed-in ANGI user." >&2
    exit 1
fi

export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"
case "$WAYLAND_DISPLAY" in
    /*) display_socket=$WAYLAND_DISPLAY ;;
    *) display_socket=${XDG_RUNTIME_DIR:-}/$WAYLAND_DISPLAY ;;
esac
if [ ! -S "$display_socket" ]; then
    echo "Google Chrome needs an active ANGI Wayland session." >&2
    exit 1
fi

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    exec dbus-run-session -- "$0" "$@"
fi

exec "$install_dir/chrome" "$@" --ozone-platform=wayland
