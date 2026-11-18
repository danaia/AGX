"""Services/updater inventory. Buildroot image candidates are checked, never live-flashed."""
import datetime
import hashlib
import json
import platform
from pathlib import Path
import subprocess
import tarfile
import time

import releases

CANDIDATE = Path('/var/lib/agx-update/candidate.tar.gz')

def updated_ago(timestamp, now=None):
    """Describe an installation time; missing dates are never inferred from age/version."""
    if isinstance(timestamp, str):
        try:
            timestamp = datetime.datetime.fromisoformat(timestamp).timestamp()
        except ValueError:
            return 'Not recorded'
    if not isinstance(timestamp, (float, int)) or timestamp <= 0:
        return 'Not recorded'
    elapsed = (time.time() if now is None else now) - timestamp
    if elapsed < -60:
        return 'Not recorded'
    if elapsed < 60:
        return 'Updated just now'
    for seconds, unit in ((86400, 'day'), (3600, 'hour'), (60, 'minute')):
        count = int(elapsed // seconds)
        if count:
            return f"Updated {count} {unit}{'s' if count != 1 else ''} ago"


def row(key, name, state, installed, status, detail, action='', timestamp=None):
    update_state = 'current' if status == 'Current' else 'update' if status == 'Source changes' else 'unknown'
    label = {'current': 'Up to date', 'update': 'Needs updating', 'unknown': 'Not checked yet'}[update_state]
    return dict(key=key, name=name, state=state, installed=installed, status=status,
                detail=detail, action=action, update_state=update_state, update_label=label,
                updated_ago=updated_ago(timestamp))


def desktop():
    current = releases.installed()
    reason = releases.guard()
    known = releases.entry(current)
    if reason:
        return row('desktop', 'ANGI desktop', 'Installed', current,
                   'Blocked', reason)
    changed = releases.tree_hash() != known.get('tree')
    return row('desktop', 'ANGI desktop', 'Installed', current,
               'Source changes' if changed else 'Current',
               'Compared with the installed release’s recorded local source. Update builds this checkout; it does not fetch Internet releases.',
               'Update' if changed else '', timestamp=known.get('time'))


def check_candidate(path=None):
    path = Path(path or CANDIDATE)
    if not path.exists():
        return 'No candidate', 'No system image has been staged from the workstation.'
    with tarfile.open(path, 'r:gz') as bundle:
        members = bundle.getmembers()
        by_name = {member.name: member for member in members}
        if len(by_name) != len(members) or set(by_name) != {'manifest.json', 'Image', 'rootfs.ext4'}:
            raise ValueError('Unexpected or duplicate system image entries')
        if any(not member.isfile() for member in members) or by_name['manifest.json'].size > 65536:
            raise ValueError('Invalid system image entries')
        manifest = json.load(bundle.extractfile(by_name['manifest.json']))
        if (manifest.get('format') != 'agx-system-update-v1'
                or manifest.get('architecture') != 'aarch64'
                or manifest.get('board') != 'raspberry-pi-5'):
            raise ValueError('System image targets a different board or format')
        for name in ('Image', 'rootfs.ext4'):
            expected = manifest['components'][name]
            if by_name[name].size != expected['bytes']:
                raise ValueError(f'{name}: size mismatch')
            digest = hashlib.sha256()
            with bundle.extractfile(by_name[name]) as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(block)
            if digest.hexdigest() != expected['sha256']:
                raise ValueError(f'{name}: checksum mismatch')
    return 'Candidate checked', (f"Staged {manifest.get('version', 'unknown version')}: kernel and root filesystem checksums match. "
                                 'Checksums do not establish publisher authenticity or freshness. Installation requires a recovery-capable image installer; live installation is unavailable.')


def runtimes():
    catalog = [('web', 'Web engine', '/var/lib/agx-data/agx-web/current'),
               ('terminal', 'OpenSSH client', '/var/lib/agx-data/agx-terminal/current'),
               ('agent', 'Herdr agent runtime', '/var/lib/agx-data/agx-agent/current'),
               ('opencode', 'OpenCode', '/var/lib/agx-data/agx-opencode/current')]
    return [row(key, name, 'Installed', path.resolve().name,
                'No version feed', 'Installed component runtime. Updates are deployed from the workstation; independent GUI installation is unavailable.', timestamp=path.lstat().st_mtime)
            for key, name, location in catalog
            if (path := Path(location)).exists()]


def base_version():
    try:
        values = dict(line.split('=', 1) for line in Path('/etc/os-release').read_text().splitlines() if '=' in line)
        return values.get('PRETTY_NAME', 'Buildroot').strip('"')
    except OSError:
        return 'Buildroot'


def inventory(verify=False, *, refresh=False):
    rows = [row('kernel', 'Linux kernel', 'Running', platform.release(),
                'Not checked', 'Kernel updates require a compatible image and reboot. Live kernel installation is unavailable.'),
            row('base', 'Base system & packages', 'Installed', base_version(),
                'Not checked', 'System libraries and dependent packages ship together in the Buildroot image; there is no apt package repository.')]
    if verify:
        try:
            status, detail = check_candidate()
        except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
            status, detail = 'Check failed', str(error)
        for item in rows:
            item.update(status=status, detail=item['detail'] + ' ' + detail)
    rows.extend(runtimes())
    for key, name, binary in [('openssl', 'OpenSSL', '/usr/lib/libssl.so.3'),
                              ('busybox', 'BusyBox system tools', '/bin/busybox'),
                              ('tmux', 'tmux', '/usr/bin/tmux'),
                              ('wpa', 'Wi-Fi supplicant', '/usr/sbin/wpa_supplicant')]:
        if Path(binary).exists():
            rows.append(row(key, name, 'Installed', '', 'Not checked', ''))
    errors = []
    try:
        rows.append(desktop())
    except (OSError, RuntimeError, ValueError) as error:
        rows.append(row('desktop', 'ANGI desktop', 'Unknown', '', 'Check failed', str(error)))
    try:
        result = subprocess.run(['/usr/bin/agx', 'services', '--json'], capture_output=True,
                                text=True, timeout=10, check=True)
        reply = json.loads(result.stdout)
        if not reply.get('ok'):
            raise RuntimeError(reply.get('message', 'Supervisor unavailable'))
        for service in reply['services']:
            name = service['name']
            rows.append(row(name, name, service['state'],
                            f"PID {service['pid']}" if service.get('pid') else '—',
                            'Image managed', 'Updated with its owning system image or component runtime. No independent version feed is configured.'))
    except (OSError, RuntimeError, ValueError, KeyError, subprocess.SubprocessError) as error:
        errors.append(f'Service inventory unavailable: {error}')
    import update_checks
    try:
        update_checks.enrich(rows, base_version(), updated_ago, refresh_now=verify or refresh)
    except (OSError, ValueError, RuntimeError) as error:
        errors.append(f'Release checks unavailable: {error}')
        for item in rows:
            item.setdefault('checked_ago', 'Not checked yet')
            item.setdefault('checked_at', None)
    import app_updates
    try:
        rows.extend(app_updates.rows(updated_ago, refresh_now=verify or refresh))
    except (OSError, ValueError, RuntimeError) as error:
        errors.append(f'Installed app checks unavailable: {error}')
    import update_restart
    update_restart.decorate(rows, update_checks.OWNERS)
    rank = {'update': 0, 'unknown': 1, 'current': 2}
    rows.sort(key=lambda item: (rank[item['update_state']], item['name'].casefold()))
    waiting = {row.get('retry_kind') for row in rows}
    notice = ('Waiting for the system clock to synchronize. Checks resume automatically.' if 'clock' in waiting
              else 'Waiting for the network. Checks will retry automatically.' if 'network' in waiting
              else 'Publisher check limit reached. Recent results are shown; checks retry when the limit resets.' if 'rate-limit' in waiting
              else 'Checks use upstream publisher releases and maintenance branches.')
    if not waiting.intersection({'clock', 'network', 'rate-limit'}):
        installable = sum(bool(item.get('action')) and not item.get('restart_pending') for item in rows)
        failed = len({item['source_url'] for item in rows if item.get('check_error')})
        notice = ('1 installable update available.' if installable == 1
                  else f'{installable} installable updates available.' if installable
                  else 'No installable updates available.')
        if failed:
            notice += f' {failed} publisher check' + ('' if failed == 1 else 's') + ' failed.'
    import install_progress
    return {'rows': rows, 'update_job': install_progress.status(), 'notice': ' '.join(errors) or notice,
            'retry_after_seconds': update_checks.retry_delay(rows)}
