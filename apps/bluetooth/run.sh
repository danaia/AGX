#!/bin/sh
# Bluetooth for ANGI, supervised by agx-init as the "bluetooth" service (installed to
# /usr/libexec/agx-bluetooth/run by buildroot/board/agx/post-build.sh).
#
# One service owns the whole chain, in order: the system D-Bus, BlueZ's bluetoothd, then
# agx-bluetoothd (the socket the desktop's Settings and `agx bluetooth` talk to). If any of them
# dies the whole chain stops and the supervisor restarts it, so the pairing agent that
# agx-bluetoothd registers with BlueZ is never left pointing at a bluetoothd that has restarted.
#
# The radio itself needs no setup here: the kernel binds the Broadcom controller once hci_uart
# loads (/etc/modules-load.d/agx-bluetooth.conf) and config.txt enables it (dtparam=krnbt=on).
data=/var/lib/agx-data
fallback=/var/lib/bluetooth

# Pairing keys outlive reboots and system updates on the data volume, which an earlier boot step
# mounts. Wait briefly for it; if it never appears, use the root filesystem so Bluetooth still works.
n=0
while ! mountpoint -q "$data"; do
    n=$((n + 1))
    [ "$n" -gt 30 ] && break
    sleep 1
done
if mountpoint -q "$data"; then
    state="$data/bluetooth"
else
    echo "AGX data volume is not mounted; keeping Bluetooth pairings on the root filesystem" >&2
    state="$fallback"
fi
umask 077
mkdir -p "$state" /run/dbus /tmp/dbus /var/lib/dbus || exit 1
chmod 700 "$state"
# System-bus clients need to traverse the socket directory. D-Bus policy
# still controls the calls they can make over the socket.
chmod 755 /run/dbus || exit 1
dbus-uuidgen --ensure 2>/dev/null

dbus=''
bluez=''
daemon=''
stop() {
    kill $daemon $bluez $dbus 2>/dev/null
    wait 2>/dev/null
}
trap 'stop; exit 0' TERM INT

# Wait up to 15 seconds for $1 (a command whose success means "ready").
wait_for() {
    i=0
    until "$@" 2>/dev/null; do
        i=$((i + 1))
        [ "$i" -gt 30 ] && return 1
        sleep 0.5
    done
}
bus_ready() { [ -S /run/dbus/system_bus_socket ]; }
bluez_ready() {
    dbus-send --system --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
        org.freedesktop.DBus.NameHasOwner string:org.bluez 2>/dev/null | grep -q 'boolean true'
}

rm -f /run/dbus/pid /run/dbus-daemon.pid
/usr/bin/dbus-daemon --system --nofork --syslog &
dbus=$!
wait_for bus_ready || { echo "System D-Bus did not start" >&2; stop; exit 1; }

STATE_DIRECTORY="$state" /usr/libexec/bluetooth/bluetoothd -n &
bluez=$!
wait_for bluez_ready || { echo "bluetoothd did not start" >&2; stop; exit 1; }

/usr/sbin/agx-bluetoothd &
daemon=$!

# Leave as soon as any member of the chain dies, so the supervisor restarts all of them together.
while kill -0 "$dbus" 2>/dev/null && kill -0 "$bluez" 2>/dev/null && kill -0 "$daemon" 2>/dev/null; do
    sleep 2
done
stop
exit 1
