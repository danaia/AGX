"""Read Agent Angi history and keep local changes to its display."""
from contextlib import closing, contextmanager
import json
import os
from pathlib import Path
import re
import sqlite3
from urllib.parse import quote

from models import atomic_write
from scope import validate_project
import work_history

CHAT_ID = re.compile(r"ses_[A-Za-z0-9]+\Z")
RUN_ID = re.compile(r"angi-[a-z0-9-]+\Z")
GENERIC_TITLE = re.compile(r"^(?:new session|untitled|session)(?:\s*[-–—].*|\s*)$", re.I)
LEAD_IN = re.compile(r"^(?:(?:please|hey angi|hi angi)[, ]+|(?:can|could|would) you\s+|help me\s+|i (?:want|need) (?:you )?to\s+)", re.I)


def title_from_prompt(prompt):
    """Turn the first substantive line into a short, locally derived subject."""
    words = " ".join(prompt.split()).strip()
    while (subject := LEAD_IN.sub("", words, count=1)) != words:
        words = subject
    if not words:
        return "Untitled"
    if len(words) > 62:
        short = words[:61].rsplit(" ", 1)[0] if " " in words[:61] else words[:61]
        words = short + "…"
    return words[0].upper() + words[1:]


@contextmanager
def database():
    """Open the pinned OpenCode database read-only, including its WAL."""
    path = Path(os.environ["XDG_DATA_HOME"]) / "opencode/opencode.db"
    if not path.is_file():
        yield None
        return
    with closing(sqlite3.connect(f"file:{quote(str(path))}?mode=ro", uri=True, timeout=2)) as connection:
        connection.row_factory = sqlite3.Row
        yield connection


def read_json(path):
    try:
        result = json.loads(path.read_text())
    except (OSError, ValueError):
        return {}
    return result if isinstance(result, dict) else {}


def saved_folder(state, session_id):
    folder = read_json(state / "chat-folders" / f"{session_id}.json").get("folder")
    return folder if isinstance(folder, str) else ""


def remember_folder(state, session_id, folder):
    if CHAT_ID.fullmatch(session_id):
        atomic_write(state / "chat-folders" / f"{session_id}.json", {"folder": folder or ""})


def chat_session(db, state, session_id):
    if db is None or not CHAT_ID.fullmatch(session_id):
        return False
    return db.execute(
        "SELECT 1 FROM session WHERE id=? AND directory=? AND parent_id IS NULL AND time_archived IS NULL",
        (session_id, str(state / "chat")),
    ).fetchone() is not None


def can_continue(state, session_id):
    with database() as db:
        return chat_session(db, state, session_id)


def chat_rows(db, state):
    if db is None:
        return []
    rows = db.execute("""
        SELECT s.id, s.title, s.time_updated,
               (SELECT json_extract(p.data, '$.text')
                FROM message AS m JOIN part AS p ON p.message_id = m.id AND p.session_id = m.session_id
                WHERE m.session_id=s.id AND json_extract(m.data, '$.role')='user'
                  AND json_extract(p.data, '$.type')='text'
                  AND COALESCE(json_extract(p.data, '$.synthetic'), 0)=0
                  AND COALESCE(json_extract(p.data, '$.ignored'), 0)=0
                ORDER BY m.time_created, m.id, p.id LIMIT 1) AS prompt
        FROM session AS s
        WHERE s.directory=? AND s.parent_id IS NULL AND s.time_archived IS NULL
        ORDER BY s.time_updated DESC
    """, (str(state / "chat"),))
    return [
        {"id": row["id"], "mode": "chat", "updated": row["time_updated"],
         "title": row["title"].strip() if row["title"] and row["title"].strip() and not GENERIC_TITLE.fullmatch(row["title"].strip())
         else title_from_prompt(row["prompt"])}
        for row in rows if isinstance(row["prompt"], str) and row["prompt"].strip()
    ]


def build_row(path):
    manifest = read_json(path)
    mode = manifest.get("mode", "project" if manifest.get("project") else "os")
    brief = manifest.get("brief")
    if mode not in ("os", "project") or not isinstance(brief, str) or not brief.strip():
        return None
    try:
        updated = path.stat().st_mtime_ns // 1_000_000
    except FileNotFoundError:
        return None
    return {"id": path.parent.name, "mode": mode, "updated": updated, "title": title_from_prompt(brief),
            "brief": brief, "folder": manifest.get("project") or ""}


def entries(state):
    builds = (build_row(path) for path in state.glob("angi-*/run.json") if RUN_ID.fullmatch(path.parent.name))
    rows = [{key: row[key] for key in ("id", "mode", "updated", "title")} for row in builds if row]
    with database() as db:
        rows.extend(chat_rows(db, state))
    visible = []
    for row in rows:
        identifier = row["id"]
        if (CHAT_ID.fullmatch(identifier) or RUN_ID.fullmatch(identifier)) and not (
                state / "history-removed" / f"{identifier}.json").exists():
            title = read_json(state / "history-titles" / f"{identifier}.json").get("title")
            visible.append({**row, "title": title if isinstance(title, str) and title else row["title"]})
    return sorted(visible, key=lambda row: (row["updated"], row["id"]), reverse=True)


def edit(state, action, entry_id, title=""):
    if not (CHAT_ID.fullmatch(entry_id) or RUN_ID.fullmatch(entry_id)):
        raise ValueError("This history entry is no longer available.")
    rows = entries(state)
    entry = next((row for row in rows if row["id"] == entry_id), None)
    if entry is None:
        raise ValueError("This history entry is no longer available.")
    if action == "remove":
        atomic_write(state / "history-removed" / f"{entry_id}.json", {"removed": True})
        return [row for row in rows if row["id"] != entry_id]
    title = " ".join(title.split())
    if not 1 <= len(title) <= 120:
        raise ValueError("Enter a title between 1 and 120 characters.")
    atomic_write(state / "history-titles" / f"{entry_id}.json", {"title": title})
    entry["title"] = title
    return rows


def request(state, args=()):
    try:
        if not args:
            return {"entries": entries(state)}
        if len(args) == 1:
            return open_entry(state, args[0])
        if (len(args) == 2 and args[0] == "remove") or (len(args) == 3 and args[0] == "rename"):
            return {"entries": edit(state, *args)}
        raise ValueError("Use agent history [ID], history rename ID TITLE, or history remove ID.")
    except ValueError as error:
        return {"error": str(error)}
    except (OSError, sqlite3.Error):
        return {"error": "History could not be updated or read. Try again."}


def transcript(db, session_id):
    rows = db.execute("""
        SELECT m.id, json_extract(m.data, '$.role') AS role, json_extract(p.data, '$.text') AS text
        FROM message AS m JOIN part AS p ON p.message_id = m.id AND p.session_id = m.session_id
        WHERE m.session_id=? AND json_extract(p.data, '$.type')='text'
          AND COALESCE(json_extract(p.data, '$.synthetic'), 0)=0
          AND COALESCE(json_extract(p.data, '$.ignored'), 0)=0
        ORDER BY m.time_created, m.id, p.id
    """, (session_id,))
    messages, previous = [], None
    for row in rows:
        if row["role"] not in ("user", "assistant") or not isinstance(row["text"], str) or not row["text"].strip():
            continue
        if row["id"] == previous:
            messages[-1]["text"] += row["text"]
        else:
            messages.append({"mine": row["role"] == "user", "text": row["text"]})
            previous = row["id"]
    return messages


def open_entry(state, entry_id):
    if (CHAT_ID.fullmatch(entry_id) or RUN_ID.fullmatch(entry_id)) and (
            state / "history-removed" / f"{entry_id}.json").exists():
        raise ValueError("This history entry is no longer available.")
    if CHAT_ID.fullmatch(entry_id):
        with database() as db:
            if chat_session(db, state, entry_id):
                folder = saved_folder(state, entry_id)
                return {"mode": "chat", "id": entry_id, "folder": folder, "messages": transcript(db, entry_id)}
    elif RUN_ID.fullmatch(entry_id):
        run = state / entry_id
        row = build_row(run / "run.json")
        if row:
            # Workspace metadata and recorded reports survive a missing OpenCode store.
            try:
                with database() as db:
                    row.update(work_history.details(run, row, db))
            except sqlite3.Error:
                row.update(work_history.details(run, row, None))
            title = read_json(state / "history-titles" / f"{entry_id}.json").get("title")
            if isinstance(title, str) and title:
                row["title"] = title
            if row["mode"] == "project":
                try:
                    row["folder"] = str(validate_project(row["folder"]))
                except ValueError as error:
                    row.update(folder="", folder_error=str(error))
            return row
    raise ValueError("This history entry is no longer available.")
