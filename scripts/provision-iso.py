#!/usr/bin/env python3
"""Prepare a private ANGI install ISO and SSH trust file from a generic ISO."""

import argparse
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from iso_media import build_iso, sha256_file, verify_iso


LOGIN_KEY = "/root/.ssh/authorized_keys"
HOST_KEY = "/etc/dropbear/dropbear_ed25519_host_key"


def checked_public_key(path):
    lines = path.read_bytes().splitlines()
    if len(lines) != 1 or not re.match(rb"^(ssh-ed25519|ssh-rsa|ecdsa-sha2-[^ ]+) ", lines[0]):
        raise ValueError("Select one OpenSSH public key")
    subprocess.run(["ssh-keygen", "-l", "-f", str(path)], stdout=subprocess.DEVNULL, check=True)
    return lines[0] + b"\n"


def personalize_rootfs(work, public_key, dropbearkey):
    login_key = work / "login.pub"
    login_key.write_bytes(public_key)
    host_key = work / "device.key"
    generated = subprocess.check_output(
        [dropbearkey, "-t", "ed25519", "-f", str(host_key)], cwd=work,
    )
    key_line = next((line for line in generated.splitlines() if line.startswith(b"ssh-ed25519 ")), None)
    if key_line is None:
        raise ValueError("Dropbear did not report the generated public host key")
    host_public_key = b" ".join(key_line.split()[:2]) + b"\n"
    commands = []
    for source, destination in ((login_key, LOGIN_KEY), (host_key, HOST_KEY)):
        commands.extend([
            f"write {source.name} {destination}",
            f"set_inode_field {destination} uid 0",
            f"set_inode_field {destination} gid 0",
            f"set_inode_field {destination} mode 0100600",
        ])
    rootfs = work / "rootfs"
    subprocess.run(["debugfs", "-w", "-f", "/dev/stdin", str(rootfs)], cwd=work,
                   input="\n".join(commands) + "\n", text=True, capture_output=True, check=True)

    # debugfs may print file-level errors without returning a failing status.
    for source, destination in ((login_key, LOGIN_KEY), (host_key, HOST_KEY)):
        content = subprocess.check_output(["debugfs", "-R", f"cat {destination}", str(rootfs)],
                                          stderr=subprocess.DEVNULL)
        metadata = subprocess.check_output(["debugfs", "-R", f"stat {destination}", str(rootfs)],
                                           stderr=subprocess.DEVNULL, text=True)
        if (content != source.read_bytes() or "Type: regular" not in metadata
                or not re.search(r"\bMode:\s+0600\b", metadata)
                or not re.search(r"User:\s+0\s+Group:\s+0\b", metadata)):
            raise ValueError(f"Could not verify the installed SSH key at {destination}")
    return host_public_key


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--sha256", required=True, help="trusted SHA-256 of the generic release ISO")
    parser.add_argument("--public-key", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path.cwd(), help="parent of the private install directory")
    parser.add_argument("--dropbearkey", default="dropbearkey", help="path to the workstation Dropbear key generator")
    args = parser.parse_args()
    generic = args.image.resolve(strict=True)
    if not re.fullmatch(r"[0-9a-fA-F]{64}", args.sha256) or sha256_file(generic) != args.sha256.lower():
        raise ValueError("The generic ISO does not match the trusted SHA-256")
    login_key = checked_public_key(args.public_key)
    keygen = shutil.which(args.dropbearkey)
    if keygen is None:
        raise ValueError("Install dropbearkey or supply its path with --dropbearkey")
    output_parent = args.output_dir.resolve(strict=True)
    with tempfile.TemporaryDirectory(dir=output_parent, prefix=".angi-provision-") as temporary:
        work = Path(temporary)
        verify_iso(generic, "generic", work)
        public_host_key = personalize_rootfs(work, login_key, str(Path(keygen).resolve()))
        with tempfile.TemporaryDirectory(dir=output_parent, prefix=".angi-install-") as install_tmp:
            staged = Path(install_tmp)
            iso = staged / "angi-rpi4-install.iso"
            build_iso(work / "boot", work / "rootfs", iso, "personalized")
            known_hosts = staged / "known_hosts"
            known_hosts.write_bytes(b"10.77.0.2,10.78.0.2 " + public_host_key)
            known_hosts.chmod(0o600)
            fingerprint = subprocess.check_output(["ssh-keygen", "-lf", str(known_hosts)], text=True).strip()
            install = Path(tempfile.mkdtemp(dir=output_parent, prefix="angi-rpi4-install-"))
            iso.replace(install / iso.name)
            known_hosts.replace(install / known_hosts.name)
    print(f"Install directory: {install}")
    print(f"Pi SSH host key: {fingerprint}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"Could not prepare the ANGI ISO: {error}")
