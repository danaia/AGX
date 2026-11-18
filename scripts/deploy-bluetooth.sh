#!/usr/bin/env bash
# Install the Bluetooth stack on an already-running development Pi without reflashing it:
# D-Bus, BlueZ, the Broadcom radio firmware, agx-bluetoothd, the supervised service script, the
# new agx-init (which defines the "bluetooth" service) and the agx CLI, plus the boot parameter
# that enables the controller (dtparam=krnbt=on).
#
# Everything is taken from the last `make build` (output/target). The new init and the boot
# parameter take effect at the next boot; this script does not reboot. Previous copies of every
# replaced file are kept in /root/agx-bluetooth-backup on the Pi.
#
# Export AGX_SSH_HOST, AGX_SSH_KEY and AGX_SSH_HOST_KEY_ALIAS first (docs/pi-ethernet.md).
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
DESTINATION=${AGX_SSH_HOST:?Export AGX_SSH_HOST (see docs/pi-ethernet.md)}
KEY=${AGX_SSH_KEY:-"${HOME}/.ssh/id_ed25519"}
SSH_OPTIONS=(-i "$KEY" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=10)
if [[ -n "${AGX_SSH_HOST_KEY_ALIAS:-}" ]]; then
    SSH_OPTIONS+=(-o "HostKeyAlias=$AGX_SSH_HOST_KEY_ALIAS" -o StrictHostKeyChecking=yes)
fi
ARCHIVE=.cache/bluetooth-stack.tar

python3 - "$ARCHIVE" <<'PY'
from pathlib import Path
import glob, sys, tarfile
root = Path('output/target')
def root_owner(entry):
    entry.uid = entry.gid = 0
    entry.uname = entry.gname = 'root'
    return entry
def need(path):
    if not (root / path).exists() and not (root / path).is_symlink():
        raise SystemExit(f'Missing {path}; run make build first')
with tarfile.open(sys.argv[1], 'w') as archive:
    for name in ['usr/bin/dbus-daemon', 'usr/bin/dbus-send', 'usr/bin/dbus-uuidgen', 'usr/bin/bluetoothctl',
                 'usr/bin/btmon', 'usr/libexec/dbus-daemon-launch-helper', 'usr/libexec/bluetooth',
                 'usr/libexec/agx-bluetooth/run', 'etc/dbus-1', 'etc/bluetooth', 'usr/share/dbus-1',
                 'etc/modules-load.d/agx-bluetooth.conf']:
        need(name)
        archive.add(root / name, arcname=name, filter=root_owner)
    libraries = glob.glob(str(root / 'usr/lib/libdbus-1.so.3*'))
    if not libraries:
        raise SystemExit('Missing libdbus-1; run make build first')
    for name in libraries:
        archive.add(name, arcname=str(Path(name).relative_to(root)), recursive=False, filter=root_owner)
    firmware = glob.glob(str(root / 'lib/firmware/brcm/BCM4345C0*.hcd'))
    if not firmware:
        raise SystemExit('Missing Bluetooth firmware; run make build first')
    for name in firmware:
        archive.add(name, arcname=str(Path(name).relative_to(root)), recursive=False, filter=root_owner)
    # Executables arrive under a temporary name and are moved into place after they are checked.
    for name in ['usr/sbin/agx-bluetoothd', 'usr/sbin/agx-init', 'usr/bin/agx']:
        need(name)
        archive.add(root / name, arcname=name + '.new', filter=root_owner)
PY

KERNEL=$(cat output/build/linux-custom/include/config/kernel.release)
REMOTE_KERNEL=$(ssh "${SSH_OPTIONS[@]}" "$DESTINATION" uname -r)
[[ "$KERNEL" == "$REMOTE_KERNEL" ]] || { echo "Kernel mismatch ($KERNEL built, $REMOTE_KERNEL running): refusing" >&2; exit 1; }

scp -O "${SSH_OPTIONS[@]}" "$ARCHIVE" "$DESTINATION:/var/lib/agx-data/bluetooth-stack.tar"
ssh "${SSH_OPTIONS[@]}" "$DESTINATION" 'sh -s' <<'REMOTE'
set -eu
backup=/root/agx-bluetooth-backup
install -d -m 700 "$backup"
for file in /usr/sbin/agx-init /usr/bin/agx /etc/passwd /etc/group /etc/shadow; do
    test ! -f "$file" || cp -p "$file" "$backup/$(basename "$file").$(date +%s)"
done
tar -xf /var/lib/agx-data/bluetooth-stack.tar -C /
rm -f /var/lib/agx-data/bluetooth-stack.tar

# D-Bus drops to an unprivileged "dbus" account; create it if the image does not have it.
if ! grep -q '^dbus:' /etc/group; then
    gid=100
    while grep -q "^[^:]*:[^:]*:$gid:" /etc/group; do gid=$((gid + 1)); done
    echo "dbus:x:$gid:" >> /etc/group
fi
gid=$(sed -n 's/^dbus:[^:]*:\([0-9]*\):.*/\1/p' /etc/group)
if ! grep -q '^dbus:' /etc/passwd; then
    uid=100
    while grep -q "^[^:]*:[^:]*:$uid:" /etc/passwd; do uid=$((uid + 1)); done
    echo "dbus:x:$uid:$gid:DBus messagebus user:/run/dbus:/bin/false" >> /etc/passwd
    if test -f /etc/shadow && ! grep -q '^dbus:' /etc/shadow; then echo 'dbus:*:0:0:99999:7:::' >> /etc/shadow; fi
fi
chown 0:"$gid" /usr/libexec/dbus-daemon-launch-helper
chmod 4750 /usr/libexec/dbus-daemon-launch-helper
chmod 755 /usr/libexec/agx-bluetooth/run /usr/libexec/bluetooth/bluetoothd

# Check each new executable runs before it replaces the old one.
chmod 755 /usr/sbin/agx-bluetoothd.new /usr/sbin/agx-init.new /usr/bin/agx.new
/usr/sbin/agx-init.new --check
/usr/bin/agx.new --version
/usr/bin/dbus-daemon --version | head -1
/usr/libexec/bluetooth/bluetoothd --version
mv /usr/sbin/agx-bluetoothd.new /usr/sbin/agx-bluetoothd
mv /usr/bin/agx.new /usr/bin/agx
# The running init keeps its old inode; the new one starts at the next boot.
mv /usr/sbin/agx-init.new /usr/sbin/agx-init

# Enable the controller at the next boot. The boot partition is not normally mounted.
if grep -q ' /dev/mmcblk0p1 ' /proc/mounts; then echo 'Boot partition already mounted; refusing' >&2; exit 1; fi
mkdir -p /run/bootcfg
mount -t vfat /dev/mmcblk0p1 /run/bootcfg
cp -p /run/bootcfg/config.txt "$backup/config.txt.$(date +%s)"
grep -qx 'dtparam=krnbt=on' /run/bootcfg/config.txt || printf '\n# Bluetooth through the kernel driver.\ndtparam=krnbt=on\n' >> /run/bootcfg/config.txt
sync
umount /run/bootcfg
rmdir /run/bootcfg
echo 'Bluetooth stack installed; reboot to enable the controller and the service.'
REMOTE
