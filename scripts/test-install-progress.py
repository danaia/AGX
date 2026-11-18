#!/usr/bin/env python3
"""Actual sandbox installs: live events, read-back verification and durable receipts."""
import importlib.util
import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'apps/agent'))
import install_progress as progress
import releases

spec = importlib.util.spec_from_file_location('release_tests', Path(__file__).with_name('test-agent-update.py'))
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)


class Progress(unittest.TestCase):
    def test_real_install_emits_stages_and_verifies_the_selected_artifact(self):
        with fixtures.Device() as device:
            device.trusted_baseline()
            stream = io.StringIO()
            def build():
                print('Compiling ANGI desktop', flush=True)
                return device.build(b'new desktop')()
            path = device.gui / 'job.json'
            self.assertEqual(progress.run(build, path=path, stream=stream), 0)
            events = [json.loads(line) for line in stream.getvalue().splitlines()]
            self.assertEqual(events[-1]['state'], 'succeeded')
            self.assertEqual(events[-1]['step'], len(progress.STAGES))
            self.assertEqual(events[-1]['release'], releases.installed())
            self.assertEqual(device.link.read_bytes(), b'new desktop')
            self.assertEqual(releases.release_name(device.link), releases.installed())
            self.assertIsNotNone(releases.entry(releases.installed()))
            self.assertTrue(any('Compiling ANGI' in event.get('log', '') for event in events))
            self.assertTrue(any(event.get('phase') == 'building' and event['indeterminate'] for event in events))
            self.assertFalse(any(event['state'] == 'succeeded' for event in events[:-1]))
            with patch.object(progress.refresh, 'desktops', return_value={42: 'old'}):
                self.assertTrue(progress.status(path)['activation_pending'])
            with patch.object(progress.refresh, 'desktops', return_value={42: releases.installed()}):
                self.assertFalse(progress.status(path)['activation_pending'])

    def test_corrupted_copy_never_selects_the_release_or_reports_success(self):
        with fixtures.Device() as device:
            device.trusted_baseline()
            previous = releases.installed()
            stream = io.StringIO()
            place = releases.place
            def corrupt(source, destination, mode='644'):
                place(source, destination, mode)
                if mode == '755':
                    Path(destination).write_bytes(b'corrupt')
            with patch.object(releases, 'place', side_effect=corrupt):
                code = progress.run(device.build(b'new'), path=device.gui / 'job.json', stream=stream)
            self.assertNotEqual(code, 0)
            self.assertEqual(releases.installed(), previous)
            self.assertFalse(any(json.loads(line)['state'] == 'succeeded' for line in stream.getvalue().splitlines()))
            self.assertEqual(progress.status(device.gui / 'job.json')['state'], 'failed')

    def test_link_verification_failure_restores_the_previous_release(self):
        with fixtures.Device() as device:
            device.trusted_baseline()
            previous = releases.installed()
            with patch.object(releases, 'point_link_at', return_value=None):
                code = progress.run(device.build(b'new'), path=device.gui / 'job.json', stream=io.StringIO())
            self.assertNotEqual(code, 0)
            self.assertEqual(releases.installed(), previous)
            self.assertEqual(releases.base(), previous)
            self.assertIn('restored', progress.status(device.gui / 'job.json')['message'])

    def test_closed_progress_view_does_not_abort_the_install(self):
        class Closed:
            def write(self, _):
                raise BrokenPipeError()
        with fixtures.Device() as device:
            device.trusted_baseline()
            path = device.gui / 'job.json'
            self.assertEqual(progress.run(device.build(b'new'), path=path, stream=Closed()), 0)
            self.assertEqual(progress.status(path)['state'], 'succeeded')
            self.assertEqual(device.link.read_bytes(), b'new')

    def test_stopped_job_has_no_success_receipt(self):
        with fixtures.Device() as device:
            path = device.gui / 'job.json'
            path.write_text(json.dumps({'format':'angi-update-job-v1', 'state':'running', 'pid':-1, 'process_token':'missing'}))
            self.assertEqual(progress.status(path)['state'], 'failed')


if __name__ == '__main__':
    unittest.main()
