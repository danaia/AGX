#!/usr/bin/env python3
"""Test local history with isolated OpenCode rows and ANGI run manifests."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "apps/agent"))
import chat
import history
import work_changes
import work_history
from work_editor import Editor


class HistoryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.home = Path(temporary.name)
        self.state = self.home / "state"
        self.folder = self.home / "project"
        self.folder.mkdir()
        self.private = str(self.state / "chat")
        self.enterContext(patch.dict(os.environ, {"HOME": str(self.home), "XDG_DATA_HOME": str(self.home)}))
        self.db_path = self.home / "opencode" / "opencode.db"
        self.db_path.parent.mkdir()
        self.db = sqlite3.connect(self.db_path)
        self.addCleanup(self.db.close)
        self.db.executescript("""
            CREATE TABLE session(id TEXT, directory TEXT, title TEXT, time_updated INTEGER, parent_id TEXT, time_archived INTEGER, time_created INTEGER);
            CREATE TABLE message(id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
            CREATE TABLE part(id TEXT, message_id TEXT, session_id TEXT, data TEXT);
        """)

    def session(self, ident, title, directory=None, parent=None, archived=None, updated=10, started=10,
                prompt="Can you help me design a window manager?"):
        self.db.execute("INSERT INTO session VALUES (?,?,?,?,?,?,?)",
                        (ident, self.private if directory is None else directory, title, updated, parent, archived, started))
        self.db.execute("INSERT INTO message VALUES (?,?,?,?)", ("m1", ident, 10, '{"role":"user"}'))
        self.part(ident, "m1", "first", prompt)
        self.db.commit()

    def part(self, session_id, message, ident, text, **options):
        self.db.execute("INSERT INTO part VALUES (?,?,?,?)",
                        (ident, message, session_id, json.dumps({"type":"text", "text":text, **options})))

    def manifest(self, name, mode, brief, folder=None):
        path = self.state / name / "run.json"
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps({"mode": mode, "brief": brief, "project": folder}))

    def test_history_lists_only_private_top_level_chat_sessions_and_builds(self):
        self.session("ses_chat1", "Window Manager Design")
        self.session("ses_fallback", "New session - 2026-11-18")
        self.session("ses_nested", "Hidden", parent="ses_chat1")
        self.session("ses_archived", "Hidden", archived=1)
        self.session("ses_other", "Hidden", directory=str(self.folder))
        self.manifest("angi-os-run", "os", "Please update the desktop diagnostics")
        self.manifest("angi-project-run", "project", "Build a recipe browser", str(self.folder))
        self.assertEqual(
            {entry["id"]: (entry["title"], entry["mode"]) for entry in history.entries(self.state)},
            {"ses_chat1": ("Window Manager Design", "chat"), "ses_fallback": ("Design a window manager?", "chat"),
             "angi-os-run": ("Update the desktop diagnostics", "os"), "angi-project-run": ("Build a recipe browser", "project")},
        )

    def test_restore_text_and_scope_after_a_new_session_is_created(self):
        self.session("ses_first", "First Chat")
        self.session("ses_latest", "Unrelated Chat", updated=30)
        self.db.execute("INSERT INTO message VALUES (?,?,?,?)", ("m2", "ses_first", 20, '{"role":"assistant"}'))
        self.part("ses_first", "m2", "p1", "Good ")
        self.part("ses_first", "m2", "p2", "to see you.")
        self.part("ses_first", "m2", "p3", "Hidden", synthetic=True)
        self.db.commit()
        history.remember_folder(self.state, "ses_first", str(self.folder))
        opened = history.open_entry(self.state, "ses_first")
        self.assertEqual(opened["folder"], str(self.folder))
        self.assertEqual(opened["messages"], [
            {"mine": True, "text": "Can you help me design a window manager?"},
            {"mine": False, "text": "Good to see you."},
        ])
        reply = '{"type":"text","sessionID":"ses_first","part":{"text":"Welcome back."}}'
        with patch.object(chat, "prepare"), patch.object(chat.subprocess, "run",
                return_value=subprocess.CompletedProcess([], 0, stdout=reply, stderr="")) as run:
            result = chat.turn(self.state, "Continue", session_id=opened["id"], folder=opened["folder"])
        self.assertEqual(result, {"session_id": "ses_first", "text": "Welcome back."})
        self.assertIn(["--session", "ses_first"], [run.call_args.args[0][i:i+2] for i in range(len(run.call_args.args[0]))])
        self.assertNotIn("--continue", run.call_args.args[0])

    def test_missing_folders_keep_the_brief_and_unrelated_sessions_cannot_be_opened(self):
        self.session("ses_other", "Outside Chat", directory=str(self.folder))
        self.manifest("angi-project-run", "project", "Build a recipe browser", str(self.folder / "gone"))
        self.assertIn("error", history.request(self.state, ["../project"]))
        self.assertIn("error", history.request(self.state, ["ses_other"]))
        self.assertFalse(history.can_continue(self.state, "ses_other"))
        with patch.object(chat, "prepare") as prepare:
            self.assertIn("error", chat.turn(self.state, "Continue", session_id="ses_other"))
            prepare.assert_not_called()
        opened = history.open_entry(self.state, "angi-project-run")
        self.assertEqual((opened["folder"], opened["brief"]), ("", "Build a recipe browser"))
        self.assertIn("does not exist", opened["folder_error"])

    def test_edits_persist_without_changing_the_original_project_or_chat(self):
        self.session("ses_first", "First Chat")
        self.manifest("angi-project-run", "project", "Build a recipe browser", str(self.folder))
        state = self.state
        self.assertEqual(history.request(state, ["rename", "angi-project-run", "  Recipe   app  "])["entries"][0]["title"],
                         "Recipe app")
        self.assertIn("error", history.request(state, ["rename", "angi-project-run", " "]))
        self.assertIn("error", history.request(state, ["remove", "../project"]))
        self.assertEqual(history.entries(state)[0]["title"], "Recipe app")
        self.assertEqual(history.open_entry(state, "angi-project-run")["brief"], "Build a recipe browser")
        self.assertEqual(history.request(state, ["remove", "angi-project-run"])["entries"][0]["id"], "ses_first")
        self.assertIn("error", history.request(state, ["angi-project-run"]))
        self.assertIn("error", history.request(state, ["rename", "angi-project-run", "Still hidden"]))
        self.assertTrue((state / "angi-project-run" / "run.json").is_file())
        self.assertTrue(self.folder.is_dir())
        self.assertEqual(history.request(state, ["remove", "ses_first"])["entries"], [])
        self.assertEqual(history.entries(state), [])
        self.assertTrue(history.can_continue(state, "ses_first"))

    def test_work_summary_uses_the_matching_run_and_links_to_existing_folders(self):
        name = "angi-project-run"
        brief = "Build a recipe browser"
        self.manifest(name, "project", brief, str(self.folder))
        path = self.state / name / "run.json"
        started = path.stat().st_mtime_ns // 1_000_000
        self.session("ses_work", "Recipe browser", str(self.folder), started=started,
                     prompt="The user wants: " + brief)
        self.session("ses_old", "Older run", str(self.folder), started=started - 60000, prompt=brief)
        self.db.execute("INSERT INTO message VALUES (?,?,?,?)",
                        ("m2", "ses_work", 20, '{"role":"assistant","finish":"stop"}'))
        self.part("ses_work", "m2", "finish", "Built the recipe browser and ran the tests.")
        self.db.commit()
        record = history.open_entry(self.state, name)
        self.assertEqual(record["summary"], "Built the recipe browser and ran the tests.")
        self.assertEqual(record["summary_label"], "Last recorded agent reply")
        self.assertEqual([r["label"] for r in record["resources"]], ["Project folder", "Run records"])
        self.session("ses_conflict", "Other run", str(self.folder), started=started, prompt=brief)
        self.assertEqual(history.open_entry(self.state, name)["summary"], "")
        output = self.folder / "site"
        output.mkdir()
        page = output / "index.html"
        page.touch()
        with patch.dict(os.environ, {"XDG_STATE_HOME": str(self.home), "AGX_SWARM_ROLE": "orchestrator"}), \
                patch.object(work_history, "current_run", return_value=path.parent):
            work_history.save("Published the starter page.", [str(page), str(output), str(output / "missing")])
        saved = history.open_entry(self.state, name)
        self.assertEqual(saved["summary"], "Published the starter page.")
        self.assertEqual(saved["summary_label"], "Work summary")
        self.assertEqual(saved["resources"][-1], {"label": "Related folder", "path": str(output)})

    def test_legacy_work_editor_retrieves_builder_patches_for_the_selected_swarm(self):
        name = "angi-project-run"
        brief = "Build a recipe browser"
        self.manifest(name, "project", brief, str(self.folder))
        started = (self.state / name / "run.json").stat().st_mtime_ns // 1_000_000
        for ident, offset, agent, prompt in [
            ("ses_leader", 1000, "angi-orchestrator", brief),
            ("ses_builder", 2000, "angi-builder", "Ready for work"),
            ("ses_next", 3000, "angi-orchestrator", "Another run"),
            ("ses_other", 4000, "angi-builder", "Ready for work"),
        ]:
            self.session(ident, agent, str(self.folder), started=started + offset, prompt=prompt)
        self.db.execute("ALTER TABLE session ADD COLUMN agent TEXT")
        self.db.execute("UPDATE session SET agent=title")
        self.db.execute("ALTER TABLE part ADD COLUMN time_created INTEGER")
        for session, file in [("ses_builder", "recipes.txt"), ("ses_other", "later.txt")]:
            patch = "@@ -1 +1 @@\n-Tea\n+Soup\n"
            metadata = {"filediff": {"file": str(self.folder / file), "patch": patch}}
            tool = {"type": "tool", "tool": "edit", "state": {"status": "completed", "metadata": metadata}}
            self.db.execute("INSERT INTO part VALUES (?,?,?,?,?)", (file, "m2", session, json.dumps(tool), 20))
        self.db.commit()
        editor = Editor(self.state, self.home, lambda _: None)
        record = editor.api("record", {"id": name})
        self.assertEqual(record["files"], [{"path": "recipes.txt", "kind": "Modified"}])
        self.assertIn("+Soup", editor.api("file", {"id": name, "path": "recipes.txt"})["patch"])

    def test_work_editor_keeps_the_recorded_diff_and_rejects_a_stale_save(self):
        name = "angi-project-run"
        self.manifest(name, "project", "Update the recipe browser", str(self.folder))
        code = self.folder / "recipes.py"
        code.write_text("version = 1\n")
        work_changes.begin(self.state / name, self.folder)
        code.write_text("version = 2\n")
        (self.folder / "recipes.txt").write_text("Soup\n")
        work_changes.finish(self.state / name)
        emitted = []
        editor = Editor(self.state, self.home, emitted.append)
        record = editor.api("record", {"id": name})
        self.assertEqual(record["root"], str(self.folder))
        self.assertEqual(record["files"], [{"path": "recipes.py", "kind": "Modified"},
                                           {"path": "recipes.txt", "kind": "Added"}])
        selection = {"id": name, "path": "recipes.py"}
        file = editor.api("file", selection)
        self.assertEqual((file["original"], file["recorded"], file["text"]),
                         ("version = 1\n", "version = 2\n", "version = 2\n"))
        save = {**selection, "revision": file["revision"], "text": "version = 3\n"}
        editor.api("save", save)
        self.assertEqual(code.read_text(), "version = 3\n")
        self.assertEqual(editor.api("file", selection)["recorded"], "version = 2\n")
        with self.assertRaisesRegex(ValueError, "changed on disk"):
            editor.api("save", save)
        editor.api("folder", selection)
        self.assertEqual(emitted, [{"folder": str(self.folder)}])
        outsider = self.home / "outside.py"
        outsider.write_text("outside\n")
        code.unlink()
        code.symlink_to(outsider)
        self.assertIsNone(editor.api("file", selection)["revision"])
        with self.assertRaises(OSError):
            editor.api("save", save)
        self.assertEqual(outsider.read_text(), "outside\n")


if __name__ == "__main__":
    unittest.main()
