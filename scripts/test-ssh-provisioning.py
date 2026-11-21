#!/usr/bin/env python3
"""Exercise key setup without using the real workstation SSH directory."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from ssh_provisioning import workstation_public_key


class WorkstationKeyTests(unittest.TestCase):
    def setUp(self):
        home = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.enterContext(patch.dict(os.environ, {"HOME": str(home)}))
        self.identity = home / ".ssh/id_ed25519"
        self.public = self.identity.with_suffix(".pub")

    def test_empty_home_gets_a_matching_pair_and_reuses_it(self):
        self.assertEqual(workstation_public_key(), self.public)
        private_bytes = self.identity.read_bytes()
        self.assertEqual(self.identity.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.identity.parent.stat().st_mode & 0o777, 0o700)
        derived = subprocess.check_output(["ssh-keygen", "-y", "-f", self.identity])
        self.assertEqual(self.public.read_bytes().split()[:2], derived.split()[:2])
        self.assertEqual(workstation_public_key(), self.public)
        self.assertEqual(self.identity.read_bytes(), private_bytes)

    def test_missing_public_key_is_recovered_from_the_private_key(self):
        workstation_public_key()
        private_bytes = self.identity.read_bytes()
        public_key = self.public.read_bytes().split()[:2]
        self.public.unlink()
        self.assertEqual(workstation_public_key(), self.public)
        self.assertEqual(self.public.read_bytes().split()[:2], public_key)
        self.assertEqual(self.identity.read_bytes(), private_bytes)

    def test_public_key_without_private_key_is_preserved(self):
        workstation_public_key()
        previous = self.public.read_bytes()
        self.identity.unlink()
        fallback = self.identity.with_name("angi_ed25519.pub")
        self.assertEqual(workstation_public_key(), fallback)
        self.assertEqual(self.public.read_bytes(), previous)
        self.assertFalse(self.identity.exists())


if __name__ == "__main__":
    unittest.main()
