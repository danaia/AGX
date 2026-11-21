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
from ssh_provisioning import checked_public_key, personalize_rootfs, workstation_public_key


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--sha256", required=True, help="trusted SHA-256 of the generic release ISO")
    parser.add_argument("--public-key", type=Path, help="OpenSSH public key (default: prepare a local Ed25519 pair)")
    parser.add_argument("--output-dir", type=Path, default=Path.cwd(), help="parent of the private install directory")
    parser.add_argument("--dropbearkey", default="dropbearkey", help="path to the workstation Dropbear key generator")
    args = parser.parse_args()
    generic = args.image.resolve(strict=True)
    if not re.fullmatch(r"[0-9a-fA-F]{64}", args.sha256) or sha256_file(generic) != args.sha256.lower():
        raise ValueError("The generic ISO does not match the trusted SHA-256")
    keygen = shutil.which(args.dropbearkey)
    if keygen is None:
        raise ValueError("Install dropbearkey or supply its path with --dropbearkey")
    output_parent = args.output_dir.resolve(strict=True)
    public = args.public_key if args.public_key is not None else workstation_public_key()
    login_key = checked_public_key(public)
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
    if args.public_key is None:
        print(f"Workstation SSH identity: {public.with_suffix('')}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"Could not prepare the ANGI ISO: {error}")
