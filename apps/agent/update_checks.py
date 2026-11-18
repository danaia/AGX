"""Persistent per-account release checks, independent of installation timestamps."""
from concurrent.futures import ThreadPoolExecutor
import fcntl
import json
import math
import os
from pathlib import Path
import platform
import tempfile
import time

import releases
import update_sources as upstream

CACHE = Path.home() / '.cache/angi/update-checks.json'
RETRY_SECONDS = 60
STARTUP_FRESH_SECONDS = 6 * 60 * 60
CLOCK_FLOOR = 1704067200  # 2024-01-01: reject the Pi’s unset boot clock before TLS.
OWNERS = {'syslog': 'busybox', 'kernel-log': 'busybox', 'cron': 'busybox',
          'console': 'busybox', 'serial-console': 'busybox', 'network': 'busybox',
          'display-access': 'desktop'}


def load(path):
    try:
        data = json.loads(path.read_text())
        if not isinstance(data, dict):
            return {}
        return {key: item for key, item in data.items()
                if isinstance(item, dict) and isinstance(item.get('endpoint'), str)
                and isinstance(item.get('checked_at'), (int, float))
                and math.isfinite(item['checked_at']) and item['checked_at'] >= 0
                and isinstance(item.get('latest', ''), str) and isinstance(item.get('error', ''), str)
                and isinstance(item.get('retry_at', 0), (int, float)) and math.isfinite(item.get('retry_at', 0))
                and isinstance(item.get('failures', 0), int) and 0 <= item.get('failures', 0) <= 6}
    except (OSError, ValueError):
        return {}


def save(path, data):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile('w', dir=path.parent, delete=False) as stream:
        temporary = stream.name
        try:
            json.dump(data, stream)
            stream.flush()
            os.fsync(stream.fileno())
            os.replace(temporary, path)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)


def clock_ready(now, cache):
    successes = [entry.get('successful_at', 0) for entry in cache.values()]
    successes = [value for value in successes if isinstance(value, (int, float)) and math.isfinite(value)]
    return now >= CLOCK_FLOOR and now + 300 >= max(successes, default=0)


def failure_kind(entry):
    if entry.get('retry_kind') in {'clock', 'network', 'rate-limit'}:
        return entry['retry_kind']
    # Recover failures written by the first checker before startup retry metadata existed.
    error = entry.get('error', '').lower()
    if 'rate limit' in error or 'check limit' in error:
        return 'rate-limit'
    if 'not yet valid' in error or 'system clock is incorrect' in error:
        return 'clock'
    if 'certificate' not in error and any(text in error for text in ('urlopen error', 'timed out', 'offline', 'network unavailable')):
        return 'network'
    return ''


def retry_due(entry, now):
    if not failure_kind(entry):
        return False
    if entry.get('clock_gated'):
        return True  # The refresh gate rechecks time without a publisher request.
    return now >= entry.get('retry_at', entry.get('checked_at', 0) + RETRY_SECONDS)


def refresh(configs, cache, get=upstream.fetch, now=None):
    """Wait for valid time, then retry transient failures without weakening TLS."""
    requested_time = now
    now = time.time() if now is None else now
    if not clock_ready(now, cache):
        return {key: {**cache.get(key, {}), 'endpoint': config['endpoint'],
                      'checked_at': cache.get(key, {}).get('checked_at', 0),
                      'error': 'Waiting for the system clock to synchronize.',
                      'retry_kind': 'clock', 'clock_gated': True, 'retry_at': 0}
                for key, config in configs.items()}

    def check(pair):
        key, config = pair
        previous = cache.get(key, {})
        age = now - previous.get('checked_at', 0)
        same_source = previous.get('endpoint') == config['endpoint']
        if same_source and not previous.get('clock_gated'):
            if previous.get('error') and not retry_due(previous, now) and 0 <= age < RETRY_SECONDS:
                return key, previous
            if previous.get('error') and now < previous.get('retry_at', 0):
                return key, previous
            if not previous.get('error') and 0 <= age < RETRY_SECONDS:
                return key, previous
        result = {'endpoint': config['endpoint'], 'checked_at': time.time() if requested_time is None else requested_time}
        try:
            result['latest'] = upstream.latest(config, get=get)
            result['successful_at'] = result['checked_at']
        except (OSError, ValueError, KeyError, TypeError, IndexError, RuntimeError) as error:
            result['error'] = str(error)[:240]
            kind = getattr(error, 'retry_kind', 'network' if isinstance(error, OSError) else '')
            if kind:
                failures = min(int(previous.get('failures', 0)) + 1, 6)
                delay = getattr(error, 'retry_after', None) or min(30 * 2 ** (failures - 1), 900)
                result.update(retry_kind=kind, failures=failures, retry_at=result['checked_at'] + delay)
            if previous.get('successful_at'):
                result['successful_at'] = previous['successful_at']
                result['last_good_version'] = previous.get('latest', previous.get('last_good_version'))
        return key, result
    with ThreadPoolExecutor(max_workers=4) as pool:
        return dict(pool.map(check, configs.items()))


def retry_delay(rows, now=None):
    now = time.time() if now is None else now
    delays = [5 if row.get('clock_gated') else max(5, min(60, math.ceil(row.get('retry_at', now + 5) - now)))
              for row in rows if row.get('retry_kind')]
    return min(delays, default=0)


def relative_checked(timestamp, relative):
    if not timestamp:
        return 'Not checked yet'
    text = relative(timestamp)
    return text.replace('Updated ', 'Last checked ', 1) if text.startswith('Updated ') else 'Check time unavailable'


def enrich(rows, base, relative, refresh_now=False, path=None, get=upstream.fetch):
    path = Path(path or CACHE)
    configs = upstream.sources(platform.release(), base)
    owners = {OWNERS.get(row['key'], row['key']) for row in rows}
    configs = {key: config for key, config in configs.items() if key in owners}
    cache = load(path)
    now = time.time()
    def wanted_checks(cached):
        if refresh_now or not clock_ready(now, cached):
            return configs
        return {key: config for key, config in configs.items()
                if cached.get(key, {}).get('endpoint') != config['endpoint']
                or retry_due(cached.get(key, {}), now)
                or (not cached.get(key, {}).get('error')
                    and now - cached.get(key, {}).get('checked_at', 0) >= STARTUP_FRESH_SECONDS)}
    if wanted_checks(cache):
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with path.with_suffix('.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            cache = load(path)
            now = time.time()
            updated = {**cache, **refresh(wanted_checks(cache), cache, get=get)}
            if updated != cache:
                save(path, updated)
            cache = updated
    versions = {}
    for owner in owners:
        try:
            if owner == 'kernel':
                versions[owner] = platform.release().split('-')[0]
            elif owner == 'base':
                import re
                versions[owner] = re.search(r'\d{4}\.\d{2}(?:\.\d+)?', base)[0]
            elif owner == 'desktop':
                versions[owner] = (releases.entry(releases.installed()) or {}).get('tree')
            else:
                versions[owner] = upstream.installed(owner)
        except (OSError, ValueError, RuntimeError, KeyError, TypeError, IndexError) as error:
            versions[owner] = None
    for row in rows:
        owner = OWNERS.get(row['key'], row['key'])
        config = configs.get(owner)
        checked = cache.get(owner, {})
        row['source_url'] = config['endpoint'] if config else ''
        # Ignore cached results from a branch/source that is no longer configured.
        if config and checked.get('endpoint') != config['endpoint']:
            checked = {}
        row['retry_kind'] = failure_kind(checked)
        row['retry_at'] = checked.get('retry_at', 0)
        row['clock_gated'] = checked.get('clock_gated', False)
        row['checked_at'] = checked.get('checked_at')
        row['checked_ago'] = relative_checked(row['checked_at'], relative)
        row['latest'] = checked.get('latest', checked.get('last_good_version', ''))
        actual = versions.get(owner)
        if actual and owner != 'desktop':
            row['installed'] = actual
        row['update_state'] = 'unknown'
        row['update_label'] = 'Not checked yet'
        if checked.get('error'):
            row['update_label'] = {'clock': 'Waiting for clock', 'network': 'Waiting for network', 'rate-limit': 'Check delayed'}.get(row['retry_kind'], 'Check failed')
            row['detail'] = checked['error']
            if row['retry_kind'] == 'rate-limit' and checked.get('last_good_version') and actual and owner != 'desktop':
                try:
                    if upstream.version(checked['last_good_version']) > upstream.version(actual):
                        row.update(update_state='update', update_label='Needs updating')
                except ValueError:
                    pass
        elif checked.get('latest'):
            if not actual:
                row['update_label'] = 'Version unavailable'
            elif owner == 'desktop':
                row['update_label'] = 'Up to date' if actual == checked['latest'] else 'Local build'
                if actual == checked['latest']:
                    row['update_state'] = 'current'
            else:
                try:
                    available = upstream.version(checked['latest']) > upstream.version(actual)
                    row['update_state'] = 'update' if available else 'current'
                    row['update_label'] = 'Needs updating' if available else 'Up to date'
                except ValueError as error:
                    row['update_label'] = 'Check failed'
                    row['detail'] = str(error)
        # A guarded local desktop build remains an explicit, separate update option.
        if row.get('action') == 'Update':
            row['update_state'] = 'update'
            row['update_label'] = 'Needs updating'
        row['check_error'] = checked.get('error', '')
    return rows
