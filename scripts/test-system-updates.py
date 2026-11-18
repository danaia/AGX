#!/usr/bin/env python3
"""Verify staged image validation and truthful desktop update availability."""
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'apps/agent'))
import system_updates as updates
import update_checks


class Updates(unittest.TestCase):
    def candidate(self, directory, *, corrupt=False, board='raspberry-pi-5', extra=False):
        payloads = {'Image': b'kernel', 'rootfs.ext4': b'root filesystem'}
        manifest = {'format': 'agx-system-update-v1', 'architecture': 'aarch64',
                    'board': board, 'version': 'test', 'components': {
                        name: {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
                        for name, data in payloads.items()}}
        if corrupt:
            payloads['Image'] = b'broken'
        payloads['manifest.json'] = json.dumps(manifest).encode()
        if extra:
            payloads['../../etc/passwd'] = b'bad'
        path = Path(directory) / 'candidate.tar.gz'
        with tarfile.open(path, 'w:gz') as archive:
            for name, data in payloads.items():
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        return path

    def test_checks_both_payloads_without_extracting(self):
        with tempfile.TemporaryDirectory() as directory:
            status, detail = updates.check_candidate(self.candidate(directory))
            self.assertEqual(status, 'Candidate checked')
            self.assertIn('live installation is unavailable', detail)
            self.assertEqual(list(Path(directory).iterdir()), [Path(directory) / 'candidate.tar.gz'])

    def test_rejects_damage_wrong_board_and_unexpected_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            for options in ({'corrupt': True}, {'board': 'other'}, {'extra': True}):
                with self.subTest(options=options), self.assertRaises(ValueError):
                    updates.check_candidate(self.candidate(directory, **options))

    def test_no_candidate_is_not_current(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(updates.check_candidate(Path(directory) / 'missing')[0], 'No candidate')

    def test_desktop_only_offers_update_for_changed_guarded_source(self):
        with patch.object(updates.releases, 'installed', return_value='release'), \
             patch.object(updates.releases, 'guard', return_value=None), \
             patch.object(updates.releases, 'entry', return_value={'tree': 'same'}), \
             patch.object(updates.releases, 'tree_hash', return_value='same') as tree:
            self.assertEqual(updates.desktop()['status'], 'Current')
            self.assertEqual(updates.desktop()['action'], '')
            tree.return_value = 'changed'
            self.assertEqual(updates.desktop()['action'], 'Update')
            with patch.object(updates.releases, 'guard', return_value='Newer release installed'):
                self.assertEqual(updates.desktop()['status'], 'Blocked')
                self.assertEqual(updates.desktop()['action'], '')

    def test_relative_install_dates_and_unknown_dates(self):
        now = 1800000000
        for age, expected in [(0, 'Updated just now'), (60, 'Updated 1 minute ago'),
                              (7200, 'Updated 2 hours ago'), (86400, 'Updated 1 day ago'),
                              (864000, 'Updated 10 days ago')]:
            self.assertEqual(updates.updated_ago(now - age, now=now), expected)
        for missing in (None, '', 'bad date', 0, now + 120):
            self.assertEqual(updates.updated_ago(missing, now=now), 'Not recorded')
        iso = updates.datetime.datetime.fromtimestamp(now - 2 * 86400, updates.datetime.timezone.utc).isoformat()
        self.assertEqual(updates.updated_ago(iso, now=now), 'Updated 2 days ago')

    def test_update_status_does_not_treat_image_integrity_as_freshness(self):
        for status in ('Candidate checked', 'No version feed', 'Blocked', 'Check failed'):
            self.assertEqual(updates.row('k', 'Kernel', '', '', status, '')['update_state'], 'unknown')
        self.assertEqual(updates.row('d', 'Desktop', '', '', 'Current', '')['update_state'], 'current')

    def test_inventory_preserves_services_and_sorts_available_updates_first(self):
        result = type('Result', (), {'stdout': json.dumps({'ok': True, 'services': [
            {'name': 'custom', 'state': 'stopped', 'pid': None},
            {'name': 'ssh', 'state': 'running', 'pid': 5}]})})()
        def report_upstream_update(rows, *args, **kwargs):
            next(row for row in rows if row['key'] == 'base')['update_state'] = 'update'
            return rows

        with patch.object(update_checks, 'enrich', side_effect=report_upstream_update), \
             patch.object(updates.Path, 'exists', return_value=False), \
             patch.object(updates, 'runtimes', return_value=[updates.row('current', 'AAA current', '', '', 'Current', '')]), \
             patch.object(updates, 'desktop', return_value=updates.row('desktop', 'ANGI desktop', '', '', 'Source changes', '', 'Update')), \
             patch.object(updates.subprocess, 'run', return_value=result):
            report = updates.inventory()
            rows = report['rows']
            self.assertEqual(report['notice'], '1 installable update available.')
            self.assertEqual({row['key'] for row in rows}, {'kernel', 'base', 'desktop', 'custom', 'ssh', 'current'})
            self.assertEqual([row['key'] for row in rows[:2]], ['desktop', 'base'])
            self.assertTrue(all(row['update_state'] == 'unknown' for row in rows[2:-1]))
            self.assertEqual(rows[-1]['key'], 'current')


if __name__ == '__main__':
    unittest.main()
