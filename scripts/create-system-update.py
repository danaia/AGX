#!/usr/bin/env python3
"""Create a reviewable AGX update candidate from a validated Buildroot image."""
import hashlib
import json
import os
from pathlib import Path
import tarfile
import tempfile

root = Path(__file__).resolve().parent.parent
images = root / 'output/images'
package_dir = root / 'output/updates'
package_dir.mkdir(parents=True, exist_ok=True)
files = {'Image': images / 'Image', 'rootfs.ext4': images / 'rootfs.ext4'}
manifest = {'format': 'agx-system-update-v1', 'architecture': 'aarch64',
            'board': 'raspberry-pi-5', 'components': {}}
for name, path in files.items():
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    manifest['components'][name] = {'bytes': path.stat().st_size, 'sha256': digest.hexdigest()}
with tempfile.TemporaryDirectory(dir=package_dir) as temporary:
    temp = Path(temporary)
    manifest_file = temp / 'manifest.json'
    manifest_file.write_text(json.dumps(manifest, indent=2) + '\n')
    archive = temp / 'agx-system-update.tar.gz'
    with tarfile.open(archive, 'w:gz', dereference=True) as bundle:
        for name, path in [('manifest.json', manifest_file), *files.items()]:
            def clean(info):
                info.uid = info.gid = 0
                info.uname = info.gname = 'root'
                info.mtime = 0
                return info
            bundle.add(path, arcname=name, recursive=False, filter=clean)
    os.replace(archive, package_dir / 'agx-system-update.tar.gz')
print(f"Prepared {package_dir / 'agx-system-update.tar.gz'}")
print('Candidate only: verification and safe on-device installation are not implemented yet.')
