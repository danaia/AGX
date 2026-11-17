"""Durable peer mail, delivered when the receiving OpenCode pane is ready."""
import json
from contextlib import contextmanager
import os
from pathlib import Path
import sqlite3
import time

ROLES = ("orchestrator", "builder", "tester")


def current_run():
    value = os.environ.get("AGX_SWARM_RUN")
    if not value:
        raise RuntimeError("Peer messaging is available inside an ANGI swarm pane.")
    run = Path(value).resolve(strict=True)
    parent = (Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm").resolve()
    if run.parent != parent or run.stat().st_uid != os.getuid():
        raise RuntimeError("Invalid swarm directory.")
    return run


@contextmanager
def database(run):
    connection = sqlite3.connect(run / "mail.sqlite", timeout=10)
    try:
        with connection:
            connection.execute("CREATE TABLE IF NOT EXISTS mail (id INTEGER PRIMARY KEY, sender TEXT, recipient TEXT, body TEXT, created REAL, delivered INTEGER DEFAULT 0)")
            yield connection
    finally:
        connection.close()


def enqueue(run, sender, recipient, body):
    if recipient not in ROLES:
        raise ValueError("Choose orchestrator, builder, or tester.")
    if not body.strip() or len(body) > 100000:
        raise ValueError("Messages must contain 1–100000 characters.")
    with database(run) as connection:
        cursor = connection.execute("INSERT INTO mail(sender,recipient,body,created) VALUES(?,?,?,?)", (sender, recipient, body, time.time()))
        return cursor.lastrowid


def send(recipient, body):
    run = current_run()
    sender = os.environ.get("AGX_SWARM_ROLE", "user")
    if sender not in (*ROLES, "user"):
        raise ValueError("Invalid sender role.")
    identifier = enqueue(run, sender, recipient, body)
    print(f"Queued message {identifier} to {recipient}.")


def inbox():
    run = current_run()
    role = os.environ.get("AGX_SWARM_ROLE", "orchestrator")
    with database(run) as connection:
        rows = connection.execute("SELECT id,sender,body,delivered FROM mail WHERE recipient=? ORDER BY id DESC LIMIT 30", (role,)).fetchall()
    print(json.dumps([dict(zip(("id", "from", "text", "delivered"), row)) for row in rows], indent=2))
