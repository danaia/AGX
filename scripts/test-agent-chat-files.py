#!/usr/bin/env python3
"""Exercise the Chat tool protocol against a disposable local folder."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SERVER = Path(__file__).resolve().parents[1] / "apps/agent/chat_files.py"


class ChatFileTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.home = Path(temporary.name).resolve()
        self.folder = self.home / "selected folder"
        self.folder.mkdir()
        self.client = subprocess.Popen([sys.executable, str(SERVER), str(self.folder)],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
                                       env={**os.environ, "HOME": str(self.home)})
        self.addCleanup(self.close_client)

    def close_client(self):
        self.client.stdin.close()
        self.client.stdout.close()
        self.client.wait(timeout=3)
        self.assertEqual(self.client.returncode, 0)

    def request(self, method, **params):
        request = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        print(json.dumps(request), file=self.client.stdin, flush=True)
        return json.loads(self.client.stdout.readline())["result"]

    def call(self, name, **arguments):
        return self.request("tools/call", name=name, arguments=arguments)

    def test_mcp_creates_directories_and_saves_and_reads_utf8_text(self):
        self.assertIn("tools", self.request("initialize")["capabilities"])
        names = {tool["name"] for tool in self.request("tools/list")["tools"]}
        self.assertEqual(names, {"mkdir", "save", "list", "read"})
        self.assertFalse(self.call("mkdir", path="notes/empty")["isError"])
        self.assertTrue((self.folder / "notes/empty").is_dir())
        self.assertFalse(self.call("save", path="notes/hello.txt", text="Café\n")["isError"])
        self.assertEqual((self.folder / "notes/hello.txt").read_text(), "Café\n")
        result = json.loads(self.call("list", path="notes")["content"][0]["text"])
        self.assertEqual(result["entries"], ["empty/", "hello.txt"])
        self.assertEqual(self.call("read", path="notes/hello.txt")["content"][0]["text"], "Café\n")

    def test_refuses_paths_outside_the_selected_folder_and_link_targets(self):
        outside = self.home / "outside"
        outside.mkdir()
        secret = outside / "file.txt"
        secret.write_text("original")
        (self.folder / "linked").symlink_to(outside, target_is_directory=True)
        (self.folder / "linked.txt").symlink_to(secret)
        (self.folder / ".private").write_text("private")
        for operation, arguments in (
            ("mkdir", {"path": "../outside/escape"}),
            ("save", {"path": str(secret), "text": "new", "overwrite": True}),
            ("save", {"path": "linked/file.txt", "text": "new", "overwrite": True}),
            ("save", {"path": "linked.txt", "text": "new", "overwrite": True}),
            ("read", {"path": "linked.txt"}),
            ("list", {"path": "linked"}),
            ("read", {"path": ".private"}),
            ("save", {"path": ".private", "text": "new", "overwrite": True}),
        ):
            with self.subTest(operation=operation, arguments=arguments):
                self.assertTrue(self.call(operation, **arguments)["isError"])
        self.assertEqual(secret.read_text(), "original")
        self.assertFalse((outside / "escape").exists())

    def test_overwrite_must_be_explicit(self):
        path = self.folder / "file.txt"
        path.write_text("original")
        self.assertTrue(self.call("save", path="file.txt", text="new")["isError"])
        self.assertEqual(path.read_text(), "original")
        self.assertFalse(self.call("save", path="file.txt", text="new", overwrite=True)["isError"])
        self.assertEqual(path.read_text(), "new")
        self.assertEqual(list(self.folder.iterdir()), [path])


if __name__ == "__main__":
    unittest.main()
