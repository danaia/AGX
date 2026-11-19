"""Private loopback bridge from ANGI Web to the authenticated desktop account."""
from functools import partial
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import mimetypes
import os
from pathlib import Path
import secrets
import sys
from threading import Lock, Thread
from urllib.parse import unquote

from history import database, open_entry
import work_changes

ASSETS = Path("/usr/share/agx/agent/work-editor")
POLICY = ("default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; "
          "font-src 'self'; img-src 'self' data:; worker-src 'self' blob:; "
          "connect-src 'self'; frame-ancestors 'none'; form-action 'none'; base-uri 'none'")
PAGE_FILES = {"index.html", "editor.css", "editor.js", "worker.js"}


class Editor:
    def __init__(self, state, assets, emit):
        self.state, self.assets, self.emit = state, assets, emit
        self.token = secrets.token_urlsafe(32)
        self.records = {}
        self.lock = Lock()

    def record(self, entry_id):
        row = open_entry(self.state, entry_id)
        if row["mode"] not in {"os", "project"}:
            raise ValueError("Choose an OS or Project work record.")
        root, evidence = None, None
        if row["mode"] == "os" or row["folder"]:
            root = work_changes.workspace(row["mode"], row["folder"])
            if root.is_dir():
                evidence = work_changes.evidence(self.state / entry_id, root)
                if evidence is None:
                    from work_history import legacy_changes
                    with database() as db:
                        evidence = legacy_changes(db, root, row)
        status = ("This folder is unavailable." if root is None or not root.is_dir()
                  else "No file changes were recorded for this run." if evidence is None
                  else "Partial history from earlier agent tool records." if evidence.get("legacy")
                  else "Some files exceeded the recording limits." if evidence["limited"]
                  else "")
        changes = evidence["files"] if evidence else {}
        return row, root, changes, status

    def api(self, route, query):
        entry_id = query.get("id")
        if not isinstance(entry_id, str):
            raise ValueError("Open a work record from Agent Angi.")
        if route == "record" or entry_id not in self.records:
            self.records[entry_id] = self.record(entry_id)
            # Keep only the evidence for recently opened tabs.
            if len(self.records) > 8:
                del self.records[next(iter(self.records))]
        row, root, changes, status = self.records[entry_id]
        if route == "record":
            card = {key: row[key] for key in ("mode", "title", "brief", "summary", "summary_label", "resources")}
            return {**card, "root": str(root or ""), "status": status,
                    "files": [{"path": path, "kind": item["kind"]} for path, item in changes.items()]}
        if route == "refresh":
            self.emit({"refresh": True})
            return {}
        path = query.get("path")
        if not isinstance(path, str):
            raise ValueError("Select a changed file or folder.")
        if route == "folder":
            allowed = {item["path"] for item in row["resources"]}
            if root and path in changes:
                path = str((root / path).parent)
                resolved = Path(path).resolve(strict=True)
                if not resolved.is_relative_to(root) or str(resolved) != path:
                    raise ValueError("This folder has moved outside the workspace.")
                allowed.add(path)
            if path not in allowed or not Path(path).is_dir():
                raise ValueError("This folder is unavailable.")
            self.emit({"folder": path})
            return {}
        if path not in changes:
            raise ValueError("This file is outside the recorded changes.")
        root = work_changes.workspace(row["mode"], row["folder"])
        if route == "file":
            return {**changes[path], **work_changes.working_copy(root, path)}
        return {"revision": work_changes.edit(root, path, query.get("text"), query.get("revision"))}


class Handler(BaseHTTPRequestHandler):
    timeout = 10
    def __init__(self, editor, *args):
        self.editor = editor
        super().__init__(*args)

    def log_message(self, *_):
        pass

    @property
    def host(self):
        return f"127.0.0.1:{self.server.server_port}"

    def respond(self, status, mime, data):
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Security-Policy", POLICY)
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = unquote(self.path).removeprefix("/") or "index.html"
        if self.headers.get("Host") == self.host and (path in PAGE_FILES or path.startswith("vs/")):
            if all(part and not part.startswith(".") for part in path.split("/")):
                try:
                    data = (self.editor.assets / path).read_bytes()
                except OSError:
                    pass
                else:
                    mime = "text/javascript" if path.endswith(".js") else mimetypes.guess_type(path)[0]
                    self.respond(200, mime or "application/octet-stream", data)
                    return
        self.respond(404, "text/plain", b"File unavailable.")

    def do_POST(self):
        token = self.headers.get("X-ANGI-Editor", "")
        route = self.path.removeprefix("/api/")
        if (self.headers.get("Host") != self.host or self.headers.get("Origin") != f"http://{self.host}"
                or not secrets.compare_digest(token.encode(), self.editor.token.encode())
                or self.headers.get_content_type() != "application/json"
                or route not in {"record", "file", "save", "folder", "refresh"}):
            self.respond(403, "text/plain", b"Open this editor from Agent Angi.")
            return
        try:
            size = int(self.headers.get("Content-Length", 0))
            if not 0 < size <= 4 * 1024 * 1024:
                raise ValueError("The editor request is too large.")
            query = json.loads(self.rfile.read(size))
            if not isinstance(query, dict):
                raise ValueError("The editor request is invalid.")
            with self.editor.lock:
                reply = self.editor.api(route, query)
        except (ValueError, OSError) as error:
            reply = {"error": str(error)}
        self.respond(200, "application/json", json.dumps(reply).encode())


def serve(state, assets=ASSETS):
    def emit(message):
        print(json.dumps(message), flush=True)

    if not (assets / "vs/loader.js").is_file():
        raise ValueError("Monaco Editor is unavailable.")
    editor = Editor(state, assets, emit)
    with ThreadingHTTPServer(("127.0.0.1", 0), partial(Handler, editor)) as server:
        # The desktop owns stdin. A close, crash or exec ends this private server.
        def watch_parent():
            sys.stdin.buffer.read()
            os._exit(0)

        Thread(target=watch_parent, daemon=True).start()
        emit({"port": server.server_port, "token": editor.token})
        server.serve_forever()
