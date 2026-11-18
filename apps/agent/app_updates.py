"""Installed apps as tracked services: each one's installed tag against its project's latest release.

Results come from the same per-account cache and retry rules as the system components, so a missing
network, an unset clock or GitHub's rate limit behave identically and never block the Services view.
"""
import time

import update_checks
import update_sources as upstream
from app_store import Store, version_label

PREFIX = 'app:'


def config(item):
    return {'endpoint': upstream.GITHUB + item['repo'] + '/releases/latest', 'kind': 'tag'}


def installed(store=None):
    """{ 'app:ID': ledger entry } for entries that name a repository."""
    return {PREFIX + key: item for key, item in (store or Store()).read().items() if isinstance(item.get('repo'), str)}


def latest_cached(path=None):
    """{id: latest tag} from the update cache, without any network access."""
    cache = update_checks.load(update_checks.CACHE if path is None else path)
    return {key[len(PREFIX):]: entry.get('latest', '') for key, entry in cache.items() if key.startswith(PREFIX)}


def rows(relative, refresh_now=False, store=None, path=None, get=upstream.fetch):
    tracked = installed(store)
    if not tracked:
        return []
    configs = {key: config(item) for key, item in tracked.items()}
    cache = update_checks.cached_checks(configs, refresh_now, path, get)
    out = []
    for key, item in sorted(tracked.items(), key=lambda pair: pair[1].get('name', pair[0]).casefold()):
        checked = cache.get(key, {})
        if checked.get('endpoint') != configs[key]['endpoint']:
            checked = {}
        latest = checked.get('latest', checked.get('last_good_version', ''))
        error = checked.get('error', '')
        retry_kind = update_checks.failure_kind(checked)
        newer = bool(latest) and latest != item['tag']
        if error and not newer:
            state, label = 'unknown', {'clock': 'Waiting for clock', 'network': 'Waiting for network',
                                       'rate-limit': 'Check delayed'}.get(retry_kind, 'Check failed')
        elif newer:
            state, label = 'update', 'Needs updating'
        elif latest:
            state, label = 'current', 'Up to date'
        else:
            state, label = 'unknown', 'Not checked yet'
        timestamp = item.get('installed_at')
        out.append(dict(
            key=key, name=item.get('name', key[len(PREFIX):]), state='Installed',
            installed=version_label(item['tag'], key[len(PREFIX):], item.get('name', key)),
            status={'update': 'Update available', 'current': 'Current'}.get(state, 'Not checked'),
            detail=error or (f"Installed from github.com/{item['repo']}. The program is replaced in place when you update; "
                             'copies that are already running keep the old version until they exit.'),
            action='Update' if newer else '', update_state=state, update_label=label,
            updated_ago=relative(timestamp),
            latest=version_label(latest, key[len(PREFIX):], item.get('name', key)) if latest else '',
            source_url=configs[key]['endpoint'],
            checked_at=checked.get('checked_at'), checked_ago=update_checks.relative_checked(checked.get('checked_at'), relative),
            retry_kind=retry_kind, retry_at=checked.get('retry_at', 0), clock_gated=checked.get('clock_gated', False),
            check_error=error))
    return out
