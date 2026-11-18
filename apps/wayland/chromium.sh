#!/bin/sh
# Launch the account's portable Chromium.
set -eu

case ${WAYLAND_DISPLAY:-} in
    /*) socket=$WAYLAND_DISPLAY ;;
    ?*) socket=${XDG_RUNTIME_DIR:+$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY} ;;
    *) socket= ;;
esac
apps=${ANGI_APPS_DIR:-/var/lib/agx-data/agx-opencode-users/$(id -u)/apps}
chromium=$apps/bin/chromium
if [ -e "$apps/pkg/chromium/current/.angi-chromium" ]; then
    chromium=$apps/pkg/chromium/current/.angi-chromium-bin
fi
if [ ! -x "$chromium" ]; then
    echo "Install Chromium from the ANGI Apps launcher first." >&2
    exit 1
fi
if [ ! -S "$socket" ]; then
    exec /usr/bin/python3 /usr/libexec/agx-wayland/session.py --request "$@"
fi

# Chromium's portable binary expects versioned udev symbols. Keep the eudev
# build private to this process tree so ANGI's input stack keeps libudev-zero.
compat=/usr/libexec/agx-wayland/lib
export LD_LIBRARY_PATH="$compat${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    exec dbus-run-session -- /usr/bin/angi-chromium "$@"
fi
if [ -n "${AGX_WAYLAND_CHROMIUM_PID:-}" ]; then
    printf '%s\n' "$$" >"$AGX_WAYLAND_CHROMIUM_PID"
    unset AGX_WAYLAND_CHROMIUM_PID
fi
exec "$chromium" --ozone-platform=wayland "$@"
