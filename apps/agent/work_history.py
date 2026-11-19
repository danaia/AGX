"""Evidence and folder links for an OS or Project run."""
import json
import os
from pathlib import Path
import sqlite3

from messaging import current_run
from models import atomic_write
from scope import OS_SOURCE


def folder(path):
    try:
        path = Path(path).expanduser().resolve(strict=True)
        return path if path.is_dir() else path.parent if path.is_file() else None
    except (OSError, RuntimeError, ValueError):
        return None


def save(summary, paths):
    run = current_run()
    if os.environ.get("AGX_SWARM_ROLE") not in ("os", "orchestrator") or not (run / "run.json").is_file():
        raise ValueError("Save the work summary from the OS agent or project orchestrator.")
    summary = summary.strip()
    if not summary or len(summary) > 12000:
        raise ValueError("Enter a work summary between 1 and 12000 characters.")
    folders = list(dict.fromkeys(str(path) for value in paths if (path := folder(value))))[:12]
    atomic_write(run / "report.json", {"summary": summary, "folders": folders})


def recorded_reply(db, directory, brief, started):
    """Match legacy runs by workspace, exact first prompt and start time."""
    if db is None or not directory:
        return ""
    rows = db.execute("""
        SELECT s.id
        FROM session s
        WHERE s.parent_id IS NULL AND s.directory=? AND s.time_created BETWEEN ? AND ?
          AND trim((SELECT json_extract(p.data,'$.text')
                    FROM message m JOIN part p ON p.session_id=m.session_id AND p.message_id=m.id
                    WHERE m.session_id=s.id AND json_extract(m.data,'$.role')='user'
                      AND json_extract(p.data,'$.type')='text'
                      AND COALESCE(json_extract(p.data,'$.synthetic'),0)=0
                      AND COALESCE(json_extract(p.data,'$.ignored'),0)=0
                    ORDER BY m.time_created,m.id,p.id LIMIT 1)) IN (?,?)
        LIMIT 2
    """, (directory, started - 10000, started + 180000, brief.strip(), "The user wants: " + brief.strip())).fetchall()
    if len(rows) != 1:
        return ""
    parts = db.execute("""
        SELECT json_extract(p.data,'$.text') FROM part p
        WHERE p.session_id=? AND json_extract(p.data,'$.type')='text'
          AND COALESCE(json_extract(p.data,'$.synthetic'),0)=0
          AND COALESCE(json_extract(p.data,'$.ignored'),0)=0
          AND p.message_id=(SELECT m.id FROM message m
              WHERE m.session_id=p.session_id AND json_extract(m.data,'$.role')='assistant'
                AND json_extract(m.data,'$.finish')='stop'
                AND EXISTS (SELECT 1 FROM part t WHERE t.session_id=m.session_id AND t.message_id=m.id
                  AND json_extract(t.data,'$.type')='text' AND trim(json_extract(t.data,'$.text'))!=''
                  AND COALESCE(json_extract(t.data,'$.synthetic'),0)=0
                  AND COALESCE(json_extract(t.data,'$.ignored'),0)=0)
              ORDER BY m.time_created DESC,m.id DESC LIMIT 1)
        ORDER BY p.id
    """, (rows[0][0],))
    text = "".join(value for value, in parts if isinstance(value, str)).strip()
    return text if len(text) <= 12000 else text[:11999].rstrip() + "…"


def details(run, row, db):
    try:
        report = json.loads((run / "report.json").read_text())
    except (OSError, ValueError):
        report = {}
    if not isinstance(report, dict):
        report = {}
    directory = str(OS_SOURCE) if row["mode"] == "os" else row["folder"]
    links = [("ANGI source folder" if row["mode"] == "os" else "Project folder", directory),
             ("Run records", str(run))]
    extra = report.get("folders")
    if isinstance(extra, list):
        links.extend(("Related folder", value) for value in extra[:12] if isinstance(value, str))
    resources = {}
    for label, value in links:
        if value and (path := folder(value)):
            resources.setdefault(str(path), {"label": label, "path": str(path)})
    summary = report.get("summary")
    if isinstance(summary, str) and summary.strip():
        label = "Work summary"
    else:
        try:
            summary = recorded_reply(db, directory, row["brief"], row["updated"])
        except sqlite3.Error:
            summary = ""
        label = "Last recorded agent reply" if summary else ""
    return {"summary": summary or "", "summary_label": label, "resources": list(resources.values())}
