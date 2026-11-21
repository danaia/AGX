"""List and mount existing ext4 NVMe partitions for ANGI's data volume."""
import fcntl
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile

DATA = Path("/var/lib/agx-data")
CONFIG = Path("/etc/agx/data-volume.uuid")
PARTITION = re.compile(r"nvme[0-9]+n[0-9]+p[0-9]+\Z")
UUID = re.compile(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}\Z")


def command(*arguments):
    return subprocess.run(arguments, capture_output=True, text=True, timeout=15, check=True).stdout


def mounted_devices():
    return {line.split()[2] for line in Path("/proc/self/mountinfo").read_text().splitlines()}


def volumes():
    mounted = mounted_devices()
    found = []
    for partition in sorted(Path("/sys/class/block").glob("nvme*n*p*")):
        if not PARTITION.fullmatch(partition.name):
            continue
        path = Path("/dev") / partition.name
        try:
            if (partition / "dev").read_text().strip() in mounted or not path.is_block_device():
                continue
            output = command("/sbin/blkid", str(path)).partition(":")[2]
            fields = dict(field.split("=", 1) for field in shlex.split(output) if "=" in field)
            uuid = fields.get("UUID", "")
            if fields.get("TYPE") == "ext4" and UUID.fullmatch(uuid):
                found.append({"path": str(path), "uuid": uuid, "label": fields.get("LABEL", ""),
                              "bytes": int((partition / "size").read_text()) * 512})
        except (OSError, ValueError, subprocess.SubprocessError):
            continue
    return found


def remember(uuid):
    with tempfile.NamedTemporaryFile(mode="w", dir=CONFIG.parent, delete=False) as record:
        pending = Path(record.name)
        try:
            record.write(uuid + "\n")
            record.flush()
            os.fsync(record.fileno())
            pending.replace(CONFIG)
        finally:
            pending.unlink(missing_ok=True)


def mount(uuid):
    if not UUID.fullmatch(uuid):
        raise ValueError("Choose an existing ext4 NVMe partition.")
    with os.fdopen(os.open("/run/agx-data-volume.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600), "r+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if DATA.is_symlink() or DATA.is_mount():
            raise RuntimeError("The ANGI data volume is already mounted or its mount point is unavailable.")
        matches = [entry for entry in volumes() if entry["uuid"] == uuid]
        if len(matches) != 1:
            raise RuntimeError("The selected partition is missing, mounted, or has a duplicate filesystem UUID.")
        DATA.mkdir(exist_ok=True)
        if any(DATA.iterdir()):
            raise RuntimeError("The ANGI data mount point contains files. Move those files before mounting.")
        command("/bin/mount", "-t", "ext4", "-o", "noatime,nosuid,nodev", matches[0]["path"], str(DATA))
        try:
            stat = DATA.stat()
            if stat.st_uid != 0 or stat.st_mode & 0o022:
                raise RuntimeError("The NVMe filesystem root must be root-owned and protected from group or other writes.")
            command("/etc/init.d/S12agx-data", "prepare")
            gui = Path("/usr/bin/agx-gui")
            if gui.is_file() and not gui.is_symlink():
                command(str(gui), "--prepare-web-storage")
            remember(uuid)
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
            command("/bin/umount", str(DATA))
            raise


def main(args):
    if os.geteuid() != 0:
        raise RuntimeError("Mounting the ANGI data volume requires administrator access.")
    if args == ["list"]:
        print(json.dumps(volumes()))
    elif len(args) == 2 and args[0] == "mount":
        mount(args[1])
    else:
        raise ValueError("Choose a data volume action.")


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except subprocess.CalledProcessError as error:
        sys.exit(error.stderr.strip() or "The system could not open the selected NVMe partition.")
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
