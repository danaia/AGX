#!/usr/bin/env python3
"""Exercise durable messaging and isolation without invoking a model."""
import concurrent.futures
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "apps/agent"))
from messaging import current_run, database, enqueue
from session import deliver_pending


class MailTests(unittest.TestCase):
    def test_delivers_only_one_message_to_a_ready_registered_peer(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            for role in ("orchestrator", "builder", "tester", "builder"):
                enqueue(run, "user", role, "Check readiness")
            live = {"agents": [{"pane_id": "w1:p1", "agent_status": "working"}, {"pane_id": "w1:p2", "agent_status": "idle"}]}
            manifest = {"session": "angi-test", "panes": {"orchestrator": "w1:p1", "builder": "w1:p2", "tester": "w1:p3"}}
            with patch("session.herdr", side_effect=[live, {}]) as api:
                deliver_pending(run, manifest)
                self.assertEqual(api.call_args.args[:4], ("angi-test", "agent", "prompt", "w1:p2"))
                self.assertEqual(api.call_count, 2)
            with database(run) as connection:
                self.assertEqual(connection.execute("SELECT delivered FROM mail ORDER BY id").fetchall(), [(0,), (1,), (0,), (0,)])

    def test_submission_failure_preserves_the_message(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            enqueue(run, "tester", "builder", "Fix the failing test")
            live = {"agents": [{"pane_id": "w1:p2", "agent_status": "done"}]}
            with patch("session.herdr", side_effect=[live, RuntimeError("Disconnected")]):
                with self.assertRaises(RuntimeError):
                    deliver_pending(run, {"session": "angi-test", "panes": {"builder": "w1:p2"}})
            with database(run) as connection:
                self.assertEqual(connection.execute("SELECT delivered FROM mail").fetchone()[0], 0)

    def test_concurrent_peer_messages_survive_reopen(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            with database(run):
                pass
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                ids = list(pool.map(lambda index: enqueue(run, "builder", "tester", f"Check {index}"), range(60)))
            self.assertEqual(len(set(ids)), 60)
            with database(run) as connection:
                self.assertEqual(connection.execute("SELECT COUNT(*) FROM mail WHERE delivered=0").fetchone()[0], 60)

    def test_rejects_invalid_recipient_and_empty_message(self):
        with tempfile.TemporaryDirectory() as temporary:
            for recipient, text in (("shell", "test"), ("builder", "  ")):
                with self.assertRaises(ValueError):
                    enqueue(Path(temporary), "user", recipient, text)

    def test_run_must_be_inside_private_state(self):
        with tempfile.TemporaryDirectory() as temporary:
            state = Path(temporary)
            run = state / "angi-swarm/run"
            run.mkdir(parents=True)
            with patch.dict(os.environ, {"XDG_STATE_HOME": str(state), "AGX_SWARM_RUN": str(run)}):
                self.assertEqual(current_run(), run)
                os.environ["AGX_SWARM_RUN"] = str(state)
                with self.assertRaises(RuntimeError):
                    current_run()


if __name__ == "__main__":
    unittest.main()
