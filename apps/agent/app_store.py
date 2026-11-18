"""Where installed apps live, and the ledger of what is installed.

Layout under the account's private NVMe store (never the small SD root filesystem):

    bin/NAME                  symlinks the terminal's PATH finds; each points through pkg/ID/current
    pkg/ID/TAG/               the unpacked release; pkg/ID/current is switched atomically on update
    installed.json            the ledger: tag, asset, checksum and time for each app
"""
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import tempfile

FORMAT = 'angi-apps-v1'
CHROMIUM_LAUNCHER = '.angi-chromium'
CHROMIUM_BINARY = '.angi-chromium-bin'


def version_label(tag, app_id, name):
    """A tag as a person reads a version: "jq-1.8.2" and "v15.2.0" become "1.8.2" and "15.2.0". Tags that
    are not version-shaped ("r42", date stamps) are shown as they are."""
    stripped = re.sub(r'^(?:%s|%s)[-_]?' % (re.escape(app_id), re.escape(name.lower())), '', tag, flags=re.I)
    stripped = re.sub(r'^v(?=\d)', '', stripped)
    return stripped if re.match(r'\d', stripped) else tag


class AppError(RuntimeError):
    """A failure whose message is fit to show to the person using the desktop."""


def default_root():
    return Path(os.environ.get('ANGI_APPS_DIR') or f'/var/lib/agx-data/agx-opencode-users/{os.getuid()}/apps')


class Store:
    def __init__(self, root=None):
        self.root = Path(root or default_root())
        self.bin = self.root / 'bin'
        self.pkg = self.root / 'pkg'
        self.ledger = self.root / 'installed.json'

    def prepare(self):
        for directory in (self.root, self.bin, self.pkg, self.root / 'tmp'):
            directory.mkdir(mode=0o755, parents=True, exist_ok=True)

    def lock(self):
        """Exclusive across processes: two installs of the same app must not interleave."""
        self.prepare()
        handle = (self.root / '.lock').open('a')
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            handle.close()
            raise AppError('Another app is being installed. Try again when it finishes.') from None
        return handle

    def read(self):
        try:
            data = json.loads(self.ledger.read_text())
        except (OSError, ValueError):
            return {}
        apps = data.get('apps') if isinstance(data, dict) and data.get('format') == FORMAT else None
        if not isinstance(apps, dict):
            return {}
        return {key: item for key, item in apps.items()
                if isinstance(item, dict) and isinstance(item.get('tag'), str) and isinstance(item.get('bins'), dict)}

    def write(self, apps):
        self.prepare()
        with tempfile.NamedTemporaryFile('w', dir=self.root, delete=False) as stream:
            temporary = stream.name
            try:
                json.dump({'format': FORMAT, 'apps': apps}, stream, indent=1)
                stream.flush()
                os.fsync(stream.fileno())
                os.chmod(temporary, 0o644)
                os.replace(temporary, self.ledger)
            finally:
                if os.path.exists(temporary):
                    os.unlink(temporary)

    def free_bytes(self):
        self.prepare()
        usage = shutil.disk_usage(self.root)
        return usage.free

    def owns(self, name, app_id):
        """True when bin/NAME is a link this app created."""
        try:
            return os.readlink(self.bin / name).startswith(f'../pkg/{app_id}/')
        except OSError:
            return False

    def link(self, name, app_id, relative):
        """Point bin/NAME at the program, replacing only a link this app already owns."""
        destination = self.bin / name
        if (destination.is_symlink() or destination.exists()) and not self.owns(name, app_id):
            raise AppError(f'A different program called {name} is already installed here.')
        temporary = self.bin / f'.{name}.{os.getpid()}'
        temporary.unlink(missing_ok=True)
        os.symlink(os.path.join('..', 'pkg', app_id, 'current', relative), temporary)
        os.replace(temporary, destination)

    def prepare_programs(self, release, app_id, programs):
        """Add ANGI's launch script beside Chromium's verified release binary."""
        if app_id != 'chromium' or 'chromium' not in programs:
            return programs
        raw = release / CHROMIUM_BINARY
        if not raw.is_symlink() or os.readlink(raw) != programs['chromium']:
            temporary = release / f'{CHROMIUM_BINARY}.{os.getpid()}'
            temporary.unlink(missing_ok=True)
            os.symlink(programs['chromium'], temporary)
            os.replace(temporary, raw)
        launcher = release / CHROMIUM_LAUNCHER
        with tempfile.NamedTemporaryFile('w', dir=release, delete=False) as stream:
            temporary = Path(stream.name)
            try:
                stream.write('#!/bin/sh\n'
                             'if [ ! -x /usr/bin/angi-chromium ]; then\n'
                             '    echo "Chromium needs the ANGI Wayland runtime." >&2\n'
                             '    exit 1\n'
                             'fi\n'
                             'exec /usr/bin/angi-chromium "$@"\n')
                stream.flush()
                temporary.chmod(0o755)
                os.replace(temporary, launcher)
            finally:
                temporary.unlink(missing_ok=True)
        return dict(programs, chromium=CHROMIUM_LAUNCHER)

    def unlink(self, name, app_id):
        if self.owns(name, app_id):
            (self.bin / name).unlink()

    def switch(self, app_id, tag):
        """Atomically make TAG the current version of the app."""
        directory = self.pkg / app_id
        temporary = directory / f'.current.{os.getpid()}'
        temporary.unlink(missing_ok=True)
        os.symlink(tag, temporary)
        os.replace(temporary, directory / 'current')

    def prune(self, app_id, keep):
        directory = self.pkg / app_id
        for entry in directory.iterdir():
            if entry.name != 'current' and entry.name != keep and not entry.name.startswith('.'):
                shutil.rmtree(entry, ignore_errors=True)
