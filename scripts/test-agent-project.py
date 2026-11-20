#!/usr/bin/env python3
"""Project mode starts one agent in the validated directory and records its history."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "apps/agent"))
import history
import project


class ProjectTests(unittest.TestCase):
    def test_launch_and_history(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary).resolve()
            folder = home / "example"
            folder.mkdir()
            state = home / "history"
            state.mkdir()
            with patch.dict(os.environ, {"HOME": str(home), "XDG_DATA_HOME": str(home / "data")}), \
                    patch.object(project, "active_selections", return_value={"builder": "lmstudio/code"}), \
                    patch.object(project, "opencode_config", side_effect=json.dumps), \
                    patch.object(project.os, "chdir") as cd, patch.object(project.os, "execvp") as run:
                project.launch(state, "Update the example", str(folder))
                cd.assert_called_once_with(folder)
                run.assert_called_once_with("opencode", ["opencode", "--agent", "angi-project", "--prompt", "Update the example"])
                config = json.loads(os.environ["OPENCODE_CONFIG_CONTENT"])
                self.assertEqual(config["angi-project"]["model"], "lmstudio/code")
                entries = history.entries(state)
                self.assertEqual(len(entries), 1)
                self.assertEqual(history.open_entry(state, entries[0]["id"])["folder"], str(folder))
                with self.assertRaises(ValueError):
                    project.launch(state, "Outside the project", str(home.parent))
                self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
