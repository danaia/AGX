#!/usr/bin/env python3
"""Model selection persistence and role configuration contract."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "apps/agent"))
import models
import session


class ModelTests(unittest.TestCase):
    def test_choices_are_private_and_apply_only_to_selected_roles(self):
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, {"XDG_STATE_HOME": temporary}):
            self.assertEqual(models.selections(), {})
            models.activate("openai")
            with patch.object(models, "catalog", return_value=["openai/example"]):
                models.save("builder", "openai/example")
            path = models.settings_path()
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            profiles = Path(temporary) / "profiles"
            profiles.mkdir()
            for role in models.ROLES:
                (profiles / f"{role}.md").write_text(role)
            with patch.object(session, "ASSETS", Path(temporary)):
                config = json.loads(session.role_config())
            self.assertEqual(config["agent"]["angi-builder"]["model"], "openai/example")
            self.assertNotIn("model", config["agent"]["angi-tester"])
            with patch.object(models, "catalog", return_value=[]):
                with self.assertRaises(ValueError):
                    models.save("tester", "openai/unavailable")
            self.assertEqual(models.selections(), {"builder": "openai/example"})

    def test_unknown_roles_and_malformed_settings_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, {"XDG_STATE_HOME": temporary}):
            models.activate("openai")
            with self.assertRaises(ValueError):
                models.save("unknown", "openai/example")
            path = models.settings_path()
            path.write_text('{"tester": 42}')
            with self.assertRaises(RuntimeError):
                models.selections()

    def roles(self, temporary):
        profiles = Path(temporary) / "profiles"
        profiles.mkdir(exist_ok=True)
        for role in models.ROLES:
            (profiles / f"{role}.md").write_text(role)
        return patch.object(session, "ASSETS", Path(temporary))

    def test_only_one_service_is_active_and_only_its_models_apply(self):
        apple = {"npm": "x", "models": {"base": {}}}
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, {"XDG_STATE_HOME": temporary, "XDG_CONFIG_HOME": temporary}):
            self.assertIsNone(models.active())
            with patch.object(models, "catalog", return_value=["openai/example"]):
                with self.assertRaises(ValueError):  # nothing is active yet
                    models.save("builder", "openai/example")
            models.activate("openai")
            with patch.object(models, "catalog", side_effect=lambda provider: [provider + "/example"]):
                models.save("builder", "openai/example")
                with self.assertRaises(ValueError):  # another service's model
                    models.save("tester", "apple/example")
            with patch("providers.apple_config", return_value=apple), self.roles(temporary):
                config = json.loads(session.role_config())
                self.assertEqual(config["agent"]["angi-builder"]["model"], "openai/example")
                self.assertNotIn("provider", config)  # Apple's server is not offered while OpenAI is active
                models.activate("apple")
                config = json.loads(session.role_config())
                self.assertNotIn("model", config["agent"]["angi-builder"])  # OpenAI's choice is kept, not used
                self.assertEqual(config["provider"], {"apple": apple})
            self.assertEqual(models.selections(), {"builder": "openai/example"})
            models.activate("openai")
            with self.roles(temporary):
                self.assertEqual(json.loads(session.role_config())["agent"]["angi-builder"]["model"], "openai/example")

    def test_existing_choices_decide_the_service_until_one_is_chosen(self):
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, {"XDG_STATE_HOME": temporary}):
            models.atomic_write(models.settings_path(), {"orchestrator": "openai/gpt"})
            self.assertEqual(models.active(), "openai")
            models.activate("apple")
            self.assertEqual(models.active(), "apple")
            self.assertEqual(models.active_selections(), {})

    def test_unknown_service_and_damaged_service_choice_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, {"XDG_STATE_HOME": temporary}):
            with self.assertRaises(ValueError):
                models.activate("claude")
            models.active_path().parent.mkdir()
            models.active_path().write_text("not json")
            with self.assertRaises(RuntimeError):
                models.active()


if __name__ == "__main__":
    unittest.main()
