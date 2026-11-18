"""Read-only upstream release sources and bounded installed-version probes.

Endpoints are fixed to the component's publisher. No release asset is executed or
installed here. Maintenance branches keep kernel/ABI changes out of patch checks.
"""
import base64
import ctypes
import json
import os
from pathlib import Path
import re
import ssl
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request

GITHUB = 'https://api.github.com/repos/'
MAX_RESPONSE = 2 * 1024 * 1024


def version(value):
    """Numeric stable versions, including OpenSSH patch levels and tmux suffixes."""
    match = re.fullmatch(r'(\d+(?:\.\d+)*)(?:p(\d+)|([a-z]))?', value)
    if not match:
        raise ValueError('Unrecognized stable version: ' + value)
    main, patch, suffix = match.groups()
    numbers = tuple(int(part) for part in main.split('.'))
    return (numbers + (0,) * (4 - len(numbers))) + (int(patch or 0), ord(suffix) - 96 if suffix else 0)


def newest(values, prefix=None):
    stable = []
    for value in values:
        try:
            rank = version(value)
        except ValueError:
            continue
        if prefix and value != prefix and not value.startswith(prefix + '.'):
            continue
        stable.append((rank, value))
    if not stable:
        raise ValueError('Publisher returned no stable release for the supported branch')
    return max(stable)[1]


class TemporarilyUnavailable(RuntimeError):
    def __init__(self, message, retry_kind, retry_after=None):
        super().__init__(message)
        self.retry_kind = retry_kind
        self.retry_after = retry_after


def fetch(url):
    parsed = urllib.parse.urlsplit(url)
    hosts = {'api.github.com', 'busybox.net', 'wpewebkit.org', 'w1.fi'}
    if parsed.scheme != 'https' or parsed.hostname not in hosts:
        raise ValueError('Unsupported release source')
    request = urllib.request.Request(url, headers={'User-Agent': 'ANGI-updater/1', 'Accept': 'application/vnd.github+json' if parsed.hostname == 'api.github.com' else '*/*'})
    try:
        with urllib.request.urlopen(request, timeout=12) as response:
            # Recheck redirects as well: release checks never follow arbitrary asset URLs.
            destination = urllib.parse.urlsplit(response.url)
            if destination.scheme != 'https' or destination.hostname not in hosts:
                raise ValueError('Unexpected publisher redirect')
            raw = response.read(MAX_RESPONSE + 1)
            if len(raw) > MAX_RESPONSE:
                raise ValueError('Release response exceeded the size limit')
            return json.loads(raw) if parsed.hostname == 'api.github.com' else raw.decode('utf-8')
    except urllib.error.HTTPError as error:
        if error.code == 429 or error.code == 403 and error.headers.get('X-RateLimit-Remaining') == '0':
            try:
                delay = float(error.headers.get('Retry-After') or float(error.headers.get('X-RateLimit-Reset', 0)) - time.time())
            except (TypeError, ValueError):
                delay = 3600
            raise TemporarilyUnavailable('Publisher check limit reached. Retrying when the limit resets.',
                                         'rate-limit', max(60, min(delay if delay > 0 else 3600, 86400))) from error
        raise RuntimeError(f'Publisher returned HTTP {error.code}') from error
    except urllib.error.URLError as error:
        reason = error.reason
        if isinstance(reason, ssl.SSLCertVerificationError):
            if getattr(reason, 'verify_code', None) == 9:
                raise TemporarilyUnavailable('Waiting for the system clock to synchronize.', 'clock') from error
            raise RuntimeError('Publisher certificate verification failed: ' + getattr(reason, 'verify_message', str(reason))) from error
        raise TemporarilyUnavailable('Network unavailable: ' + str(reason), 'network') from error
    except TimeoutError as error:
        raise TemporarilyUnavailable('Publisher connection timed out.', 'network') from error


def source(repo, kind='release', suffix='', prefix=None):
    endpoint = GITHUB + repo + ('/releases/latest' if kind == 'release' else suffix)
    return dict(endpoint=endpoint, kind=kind, prefix=prefix)


def sources(kernel_version, base_version):
    kernel_series = '.'.join(kernel_version.split('.')[:2])
    if not re.fullmatch(r'\d+\.\d+', kernel_series):
        raise ValueError('Cannot identify the running kernel maintenance branch')
    base_match = re.search(r'\d{4}\.\d{2}', base_version)
    base_series = base_match[0] if base_match else '2026.08'
    return {
        'kernel': source('raspberrypi/linux', 'kernel', '/contents/Makefile?ref=rpi-' + kernel_series + '.y', kernel_series),
        'base': source('buildroot/buildroot', 'refs', '/git/matching-refs/tags/' + base_series, base_series),
        'desktop': source('danaia/ANGI', 'commit', '/commits/main'),
        'agent': source('herdrdev/herdr'),
        'opencode': source('anomalyco/opencode'),
        'tailscale': source('tailscale/tailscale'),
        'ssh': source('mkj/dropbear', 'dropbear', '/git/matching-refs/tags/DROPBEAR_'),
        'bluetooth': source('bluez/bluez', 'refs', '/git/matching-refs/tags/5.'),
        'openssl': source('openssl/openssl', 'openssl', '/git/matching-refs/tags/openssl-3.6.'),
        'terminal': source('openssh/openssh-portable', 'openssh', '/git/matching-refs/tags/V_'),
        'tmux': source('tmux/tmux'),
        'busybox': dict(endpoint='https://busybox.net/downloads/', kind='busybox'),
        'web': dict(endpoint='https://wpewebkit.org/releases/', kind='webkit'),
        'wpa': dict(endpoint='https://w1.fi/releases/', kind='wpa'),
    }


def latest(config, get=fetch):
    data = get(config['endpoint'])
    kind = config['kind']
    if kind == 'release':
        if data.get('draft') or data.get('prerelease'):
            raise ValueError('Publisher returned a preview release')
        tag = data['tag_name'].removeprefix('v')
        version(tag)
        return tag
    if kind == 'tag':
        # Installed apps keep whatever tag scheme their project uses ("v2.1", "15.2.0", "r42"), so this
        # is compared for equality by the app updater rather than parsed as a version.
        if data.get('draft') or data.get('prerelease'):
            raise ValueError('Publisher returned a preview release')
        tag = data['tag_name']
        if not isinstance(tag, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,63}', tag):
            raise ValueError('Unrecognized release tag')
        return tag
    if kind == 'commit':
        tree = data['commit']['tree']['sha']
        if not re.fullmatch('[0-9a-f]{40}', tree):
            raise ValueError('Invalid upstream source tree')
        return tree
    if kind == 'kernel':
        makefile = base64.b64decode(data['content'], validate=False).decode('utf-8')
        fields = [re.search(r'^' + name + r'\s*=\s*(\d+)\s*$', makefile, re.M)[1]
                  for name in ('VERSION', 'PATCHLEVEL', 'SUBLEVEL')]
        result = '.'.join(fields)
        if config.get('prefix') and not result.startswith(config['prefix'] + '.'):
            raise ValueError('Kernel response does not match its maintenance branch')
        return result
    if kind in {'refs', 'dropbear', 'openssl', 'openssh'}:
        tags = [item['ref'].removeprefix('refs/tags/') for item in data]
        if kind == 'dropbear':
            tags = [tag.removeprefix('DROPBEAR_') for tag in tags]
        elif kind == 'openssl':
            tags = [tag.removeprefix('openssl-') for tag in tags]
        elif kind == 'openssh':
            tags = [re.sub(r'^V_(\d+)_(\d+)_P(\d+)$', r'\1.\2p\3', tag) for tag in tags]
        return newest(tags, config.get('prefix'))
    patterns = {'busybox': r'busybox-(\d+\.\d+\.\d+)\.tar\.bz2',
                'webkit': r'wpewebkit-(\d+\.\d+\.\d+)\.tar\.xz',
                'wpa': r'wpa_supplicant-(\d+\.\d+)\.tar\.gz'}
    candidates = re.findall(patterns[kind], data)
    if kind == 'webkit':
        candidates = [tag for tag in candidates if int(tag.split('.')[1]) % 2 == 0]
    return newest(candidates)


def probe(command, pattern):
    try:
        process = subprocess.run(command, capture_output=True, text=True, timeout=5,
                                 env={**os.environ, 'LC_ALL': 'C'}, check=False)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError('Installed version command timed out') from error
    if process.returncode:
        raise RuntimeError('Installed version command failed')
    match = re.search(pattern, process.stdout + process.stderr)
    if not match:
        raise ValueError('Installed version could not be read')
    return match[1]


def installed(key):
    commands = {
        'agent': (['/usr/bin/herdr', '--version'], r'herdr (\d+\.\d+\.\d+)'),
        'opencode': (['/usr/libexec/opencode/opencode', '--version'], r'^(\d+\.\d+\.\d+)'),
        'tailscale': (['/usr/bin/tailscale', 'version'], r'^(\d+\.\d+\.\d+)'),
        'ssh': (['/usr/sbin/dropbear', '-V'], r'Dropbear v(\d+\.\d+)'),
        'bluetooth': (['/usr/libexec/bluetooth/bluetoothd', '--version'], r'^(\d+\.\d+)'),
        'terminal': (['/usr/bin/ssh', '-V'], r'OpenSSH_(\d+\.\d+p\d+)'),
        'tmux': (['/usr/bin/tmux', '-V'], r'tmux (\d+\.\d+[a-z]?)'),
        'busybox': (['/bin/busybox', '--help'], r'BusyBox v(\d+\.\d+\.\d+)'),
        'wpa': (['/usr/sbin/wpa_supplicant', '-v'], r'wpa_supplicant v(\d+\.\d+)'),
    }
    if key == 'openssl':
        return re.search(r'OpenSSL (\d+\.\d+\.\d+)', ssl.OPENSSL_VERSION)[1]
    if key == 'web':
        # Ask the installed engine itself, rather than assuming the checkout's pin was deployed.
        lib = ctypes.CDLL('/usr/lib/libWPEWebKit-2.0.so.1')
        return '.'.join(str(getattr(lib, 'webkit_get_' + part + '_version')()) for part in ('major', 'minor', 'micro'))
    return probe(*commands[key])
