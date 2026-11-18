#!/usr/bin/env python3
"""Distinguish activation requirements from restarts actually pending on this account."""
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'apps/agent'))
import update_restart as restart
from update_checks import OWNERS


class Restart(unittest.TestCase):
    def decorate(self, key, running=None):
        with patch.object(restart.releases, 'installed', return_value='new'), \
             patch.object(restart.refresh, 'desktops', return_value=running or {}):
            return restart.decorate([{'key': key, 'action': ''}], OWNERS)[0]

    def test_system_image_updates_require_a_reboot_without_claiming_one_is_pending(self):
        for key in ('kernel', 'base', 'openssl', 'busybox', 'network', 'display-access'):
            row = self.decorate(key)
            self.assertEqual(row['restart_kind'], 'computer', key)
            self.assertFalse(row['restart_pending'], key)
            self.assertIn('not available', row['install_note'])

    def test_service_and_application_impact_is_scoped(self):
        for key, kind in [('bluetooth', 'service'), ('tailscale', 'service'),
                          ('ssh', 'service'), ('wpa', 'service'),
                          ('web', 'application'), ('tmux', 'application'),
                          ('terminal', 'application'), ('agent', 'application')]:
            self.assertEqual(self.decorate(key)['restart_kind'], kind)
        self.assertIn('disconnect', self.decorate('bluetooth')['restart_detail'])
        self.assertIn('connections', self.decorate('tailscale')['restart_detail'])
        self.assertIn('commands running', self.decorate('tmux')['restart_detail'])

    def test_only_a_running_older_desktop_proves_a_restart_is_pending(self):
        self.assertFalse(self.decorate('desktop', {})['restart_pending'])
        self.assertFalse(self.decorate('desktop', {10: 'new'})['restart_pending'])
        self.assertTrue(self.decorate('desktop', {10: 'old'})['restart_pending'])
        self.assertFalse(self.decorate('kernel', {10: 'old'})['restart_pending'])
        self.assertFalse(self.decorate('display-access', {10: 'old'})['restart_pending'])


if __name__ == '__main__':
    unittest.main()
