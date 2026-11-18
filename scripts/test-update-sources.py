#!/usr/bin/env python3
"""Version/feed semantics, persistent timestamps, and failures without network access."""
import base64
import json
from pathlib import Path
import sys
import ssl
import urllib.error
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'apps/agent'))
import update_sources as sources
import update_checks as checks
import system_updates as inventory


class Sources(unittest.TestCase):
    def test_numeric_versions_and_stable_release_filtering(self):
        self.assertEqual(sources.newest(['5.9', '5.88', '5.87']), '5.88')
        self.assertEqual(sources.newest(['2026.08', '2026.08.2', '2026.11-rc1'], '2026.08'), '2026.08.2')
        self.assertGreater(sources.version('10.6p2'), sources.version('10.6p1'))
        self.assertGreater(sources.version('3.6b'), sources.version('3.6a'))
        self.assertEqual(sources.version('1.2'), sources.version('1.2.0'))
        for tag in ('3.7.0-beta1', 'v3.6.0', '3.6.0-rc1'):
            with self.assertRaises(ValueError):
                sources.version(tag)

    def test_tag_and_release_endpoints(self):
        catalog = sources.sources('6.12.61-v8-16k', 'Buildroot 2026.08.1')
        self.assertIn('rpi-6.12.y', catalog['kernel']['endpoint'])
        self.assertIn('2026.08', catalog['base']['endpoint'])
        cases = [
            ('ssh', [{'ref': 'refs/tags/DROPBEAR_2026.94'}], '2026.94'),
            ('bluetooth', [{'ref': 'refs/tags/5.9'}, {'ref': 'refs/tags/5.88'}], '5.88'),
            ('openssl', [{'ref': 'refs/tags/openssl-3.6.5'}, {'ref': 'refs/tags/openssl-3.6.0-beta1'}], '3.6.5'),
            ('terminal', [{'ref': 'refs/tags/V_10_6_P2'}], '10.6p2'),
            ('agent', {'tag_name': 'v0.9.5', 'prerelease': False}, '0.9.5'),
            ('web', 'wpewebkit-2.55.1.tar.xz wpewebkit-2.54.2.tar.xz', '2.54.2'),
            ('kernel', {'content': base64.b64encode(b'VERSION = 6\nPATCHLEVEL = 12\nSUBLEVEL = 118\n').decode()}, '6.12.118'),
        ]
        for key, payload, expected in cases:
            with self.subTest(key=key):
                self.assertEqual(sources.latest(catalog[key], get=lambda _: payload), expected)
        with self.assertRaises(ValueError):
            sources.latest(catalog['agent'], get=lambda _: {'tag_name': 'v0.10.0', 'prerelease': True})

    def test_release_attempts_and_errors_keep_distinct_timestamps(self):
        config = {'agent': sources.source('herdrdev/herdr')}
        first = checks.refresh(config, {}, get=lambda _: {'tag_name': 'v0.9.5'}, now=1800000000)
        self.assertEqual(first['agent']['checked_at'], 1800000000)
        self.assertEqual(first['agent']['successful_at'], 1800000000)
        def fail(_):
            raise OSError('offline')
        cached = checks.refresh(config, first, get=fail, now=1800000020)
        self.assertEqual(cached, first)
        failed = checks.refresh(config, first, get=fail, now=1800000100)
        self.assertEqual(failed['agent']['checked_at'], 1800000100)
        self.assertEqual(failed['agent']['successful_at'], 1800000000)
        self.assertEqual(failed['agent']['error'], 'offline')
        self.assertNotIn('latest', failed['agent'])

    def test_enrichment_uses_persisted_check_time_and_actual_installed_version(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'checks.json'
            config = sources.sources('6.12.61', 'Buildroot 2026.08.1')['agent']
            stamp = checks.time.time()
            checks.save(path, {'agent': {'endpoint': config['endpoint'], 'checked_at': stamp, 'latest': '0.9.6'}})
            row = inventory.row('agent', 'Herdr', '', 'runtime-hash', 'No version feed', '', timestamp=500)
            with patch.object(sources, 'installed', return_value='0.9.5'), patch.object(checks.platform, 'release', return_value='6.12.61'):
                result = checks.enrich([row], 'Buildroot 2026.08.1', lambda t: f'Updated {t}', path=path)[0]
            self.assertEqual(result['installed'], '0.9.5')
            self.assertEqual(result['update_label'], 'Needs updating')
            self.assertEqual(result['checked_ago'], f'Last checked {stamp}')
            self.assertEqual(result['updated_ago'], inventory.updated_ago(500))
            self.assertEqual(result['action'], '')  # Release availability does not invent an installer.
            stored = json.loads(path.read_text())
            stored['agent']['error'] = 'offline'
            checks.save(path, stored)
            def offline(_):
                raise OSError('offline')
            with patch.object(sources, 'installed', return_value='0.9.5'), patch.object(checks.platform, 'release', return_value='6.12.61'):
                result = checks.enrich([row], 'Buildroot 2026.08.1', inventory.updated_ago, path=path, get=offline)[0]
            self.assertEqual(result['update_label'], 'Waiting for network')
            self.assertEqual(result['update_state'], 'unknown')

    def test_unset_boot_clock_waits_without_https_or_fake_check_timestamps(self):
        config = {'agent': sources.source('herdrdev/herdr')}
        cached = {'agent': {'endpoint': config['agent']['endpoint'], 'checked_at': 1800000000,
                            'successful_at': 1800000000, 'latest': '0.9.5'}}
        calls = []
        def get(url):
            calls.append(url)
            return {'tag_name': 'v0.9.6'}
        waiting = checks.refresh(config, cached, get=get, now=6903)
        self.assertEqual(calls, [])
        self.assertEqual(waiting['agent']['checked_at'], 1800000000)
        self.assertEqual(waiting['agent']['successful_at'], 1800000000)
        self.assertTrue(waiting['agent']['clock_gated'])
        recovered = checks.refresh(config, waiting, get=get, now=1800000020)
        self.assertEqual(len(calls), 1)
        self.assertEqual(recovered['agent']['latest'], '0.9.6')
        self.assertNotIn('error', recovered['agent'])

    def test_cached_startup_failure_recovers_during_inventory_without_manual_check(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'cache'
            catalog = sources.sources('6.12.61', 'Buildroot 2026.08.1')
            checks.save(path, {'agent': {'endpoint': catalog['agent']['endpoint'], 'checked_at': 6903,
                'successful_at': 1794973492, 'last_good_version': '0.9.5',
                'error': 'certificate is not yet valid or the system clock is incorrect'}})
            row = inventory.row('agent', 'Herdr', '', '', '', '')
            with patch.object(checks.time, 'time', return_value=1800000000), \
                 patch.object(checks.platform, 'release', return_value='6.12.61'), \
                 patch.object(sources, 'installed', return_value='0.9.5'):
                checks.enrich([row], 'Buildroot 2026.08.1', inventory.updated_ago, path=path,
                              get=lambda _: {'tag_name': 'v0.9.5'})
            self.assertEqual(row['update_label'], 'Up to date')
            self.assertEqual(row['checked_at'], 1800000000)
            self.assertEqual(checks.retry_delay([row]), 0)
            self.assertNotIn('error', checks.load(path)['agent'])

    def test_first_boot_waits_then_checks_automatically_after_time_sync(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'cache'
            row = inventory.row('agent', 'Herdr', '', '', '', '')
            calls = []
            def get(url):
                calls.append(url)
                return {'tag_name': 'v0.9.5'}
            with patch.object(checks.time, 'time', return_value=6903), \
                 patch.object(checks.platform, 'release', return_value='6.12.61'), \
                 patch.object(sources, 'installed', return_value='0.9.5'):
                checks.enrich([row], 'Buildroot 2026.08.1', inventory.updated_ago, path=path, get=get, refresh_now=True)
                self.assertEqual(row['update_label'], 'Waiting for clock')
                self.assertEqual(row['checked_ago'], 'Not checked yet')
                self.assertEqual(checks.retry_delay([row]), 5)
            self.assertEqual(calls, [])
            self.assertTrue(checks.load(path)['agent']['clock_gated'])
            with patch.object(checks.time, 'time', return_value=1800000000), \
                 patch.object(checks.platform, 'release', return_value='6.12.61'), \
                 patch.object(sources, 'installed', return_value='0.9.5'):
                checks.enrich([row], 'Buildroot 2026.08.1', inventory.updated_ago, path=path, get=get)
            self.assertEqual(len(calls), 1)
            self.assertEqual(row['update_label'], 'Up to date')
            self.assertEqual(row['checked_at'], 1800000000)
            self.assertEqual(checks.retry_delay([row]), 0)

    def test_network_retries_back_off_then_recover(self):
        config = {'agent': sources.source('herdrdev/herdr')}
        calls = []
        def get(url):
            calls.append(url)
            raise sources.TemporarilyUnavailable('offline', 'network')
        first = checks.refresh(config, {}, get=get, now=1800000000)
        self.assertEqual(first['agent']['retry_at'], 1800000030)
        self.assertEqual(checks.refresh(config, first, get=get, now=1800000010), first)
        self.assertEqual(len(calls), 1)
        second = checks.refresh(config, first, get=get, now=1800000030)
        self.assertEqual(second['agent']['retry_at'], 1800000090)
        good = checks.refresh(config, second, get=lambda _: {'tag_name': 'v0.9.5'}, now=1800000090)
        self.assertNotIn('error', good['agent'])
        self.assertNotIn('retry_kind', good['agent'])

    def test_certificate_failures_stay_verified(self):
        url = sources.source('herdrdev/herdr')['endpoint']
        for code, clock_error in [(9, True), (10, False), (20, False)]:
            reason = ssl.SSLCertVerificationError(1, 'certificate failed')
            reason.verify_code = code
            reason.verify_message = 'certificate failed'
            with patch.object(sources.urllib.request, 'urlopen', side_effect=urllib.error.URLError(reason)):
                if clock_error:
                    with self.assertRaises(sources.TemporarilyUnavailable) as caught:
                        sources.fetch(url)
                    self.assertEqual(caught.exception.retry_kind, 'clock')
                else:
                    with self.assertRaises(RuntimeError) as caught:
                        sources.fetch(url)
                    self.assertNotIsInstance(caught.exception, sources.TemporarilyUnavailable)

    def test_recent_success_is_reused_on_startup_but_manual_checks_can_refresh(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'cache'
            row = inventory.row('agent', 'Herdr', '', '', '', '')
            config = sources.sources('6.12.61', 'Buildroot 2026.08.1')['agent']
            checks.save(path, {'agent': {'endpoint': config['endpoint'], 'checked_at': 1800000000, 'latest':'0.9.5'}})
            calls = []
            def get(url):
                calls.append(url)
                return {'tag_name':'v0.9.6'}
            with patch.object(checks.time, 'time', return_value=1800000100), \
                 patch.object(checks.platform, 'release', return_value='6.12.61'), \
                 patch.object(sources, 'installed', return_value='0.9.5'):
                checks.enrich([row], 'Buildroot 2026.08.1', inventory.updated_ago, path=path, get=get)
                self.assertEqual(calls, [])
                checks.enrich([row], 'Buildroot 2026.08.1', inventory.updated_ago, path=path, get=get, refresh_now=True)
            self.assertEqual(len(calls), 1)
            self.assertEqual(row['update_label'], 'Needs updating')

    def test_publisher_rate_limit_uses_server_reset_and_keeps_known_update(self):
        url = sources.source('herdrdev/herdr')['endpoint']
        error = urllib.error.HTTPError(url, 403, 'limited', {'X-RateLimit-Remaining':'0', 'X-RateLimit-Reset':'1800003600'}, None)
        with patch.object(sources.time, 'time', return_value=1800000000), \
             patch.object(sources.urllib.request, 'urlopen', side_effect=error):
            with self.assertRaises(sources.TemporarilyUnavailable) as caught:
                sources.fetch(url)
        self.assertEqual(caught.exception.retry_kind, 'rate-limit')
        self.assertEqual(caught.exception.retry_after, 3600)
        config = {'agent': sources.source('herdrdev/herdr')}
        cached = {'agent': {'endpoint': url, 'checked_at':1799999000, 'successful_at':1799999000, 'latest':'0.9.6'}}
        def limited(_):
            raise sources.TemporarilyUnavailable('Publisher check limit reached', 'rate-limit', 3600)
        failed = checks.refresh(config, cached, get=limited, now=1800000000)
        self.assertEqual(failed['agent']['retry_at'], 1800003600)
        self.assertEqual(failed['agent']['last_good_version'], '0.9.6')
        calls = []
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'cache'
            checks.save(path, failed)
            row = inventory.row('agent', 'Herdr', '', '', '', '')
            with patch.object(checks.time, 'time', return_value=1800000100), \
                 patch.object(checks.platform, 'release', return_value='6.12.61'), \
                 patch.object(sources, 'installed', return_value='0.9.5'):
                checks.enrich([row], 'Buildroot 2026.08.1', inventory.updated_ago, path=path, get=lambda u: calls.append(u))
            self.assertEqual(calls, [])
            self.assertEqual(row['update_label'], 'Needs updating')
            self.assertEqual(row['retry_kind'], 'rate-limit')

    def test_damaged_cache_records_are_discarded(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'cache'
            path.write_text(json.dumps({'broken': 'record', 'bad-time': {'endpoint': 'x', 'checked_at': 'yesterday'}}))
            self.assertEqual(checks.load(path), {})

    def test_owner_services_share_one_endpoint_check(self):
        with tempfile.TemporaryDirectory() as temporary:
            rows = [inventory.row(key, key, '', '', '', '') for key in ('syslog', 'cron')]
            calls = []
            def get(url):
                calls.append(url)
                return 'busybox-1.38.0.tar.bz2'
            with patch.object(sources, 'installed', return_value='1.38.0'), patch.object(checks.platform, 'release', return_value='6.12.61'):
                checks.enrich(rows, 'Buildroot 2026.08.1', inventory.updated_ago, path=Path(temporary) / 'cache', refresh_now=True, get=get)
            self.assertEqual(calls, ['https://busybox.net/downloads/'])
            self.assertTrue(all(row['update_label'] == 'Up to date' for row in rows))
            self.assertEqual(rows[0]['checked_at'], rows[1]['checked_at'])


if __name__ == '__main__':
    unittest.main()
