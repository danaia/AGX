#!/usr/bin/env python3
"""Extract site icons for the ANGI Web bookmark bar from a local Chrome profile.

Usage: extract-favicons.py PROFILE_DIR [OUT_DIR]

Reads the profile's `Favicons` database (a copy, so Chrome may stay open) and writes one
`<host>.png` per bookmarked site into OUT_DIR (default ~/.config/agx/favicons). Everything
stays on this machine: no icon is fetched from the network. Copy OUT_DIR to the same path
on the ANGI device. Sites Chrome has no icon for are skipped; ANGI shows a letter instead.
"""
import json, os, re, shutil, sqlite3, sys, tempfile
from urllib.parse import urlsplit

MAX_BYTES = 64 * 1024
SAFE = re.compile(r"^[a-z0-9.-]{1,253}$")

def host_of(url):
    host = (urlsplit(url).hostname or "").lower()
    return host[4:] if host.startswith("www.") else host

def bookmark_hosts(profile):
    hosts = set()
    def walk(node):
        if node.get("type") == "url" or "url" in node:
            hosts.add(host_of(node.get("url", "")))
        for child in node.get("children", []):
            walk(child)
    for name in ("AccountBookmarks", "Bookmarks"):
        path = os.path.join(profile, name)
        if os.path.isfile(path):
            roots = json.load(open(path)).get("roots", {})
            for root in roots.values():
                if isinstance(root, dict):
                    walk(root)
    hosts.discard("")
    return {h for h in hosts if SAFE.match(h)}

def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    profile = os.path.expanduser(sys.argv[1])
    out = os.path.expanduser(sys.argv[2] if len(sys.argv) > 2 else "~/.config/agx/favicons")
    wanted = bookmark_hosts(profile)
    with tempfile.TemporaryDirectory() as tmp:
        copy = os.path.join(tmp, "Favicons")
        shutil.copyfile(os.path.join(profile, "Favicons"), copy)
        db = sqlite3.connect(copy)
        rows = db.execute(
            "SELECT m.page_url, b.width, b.image_data FROM icon_mapping m "
            "JOIN favicon_bitmaps b ON b.icon_id = m.icon_id WHERE b.image_data IS NOT NULL"
        )
        best = {}
        for url, width, data in rows:
            host = host_of(url)
            if host not in wanted or not data.startswith(b"\x89PNG") or len(data) > MAX_BYTES:
                continue
            # Prefer 32px, then the largest below it, then anything else.
            rank = (width == 32, width < 32 and width, -width)
            if host not in best or rank > best[host][0]:
                best[host] = (rank, data)
    os.makedirs(out, mode=0o700, exist_ok=True)
    for host, (_, data) in best.items():
        with open(os.path.join(out, host + ".png"), "wb") as f:
            f.write(data)
    print(f"{len(best)} icons for {len(wanted)} bookmarked sites -> {out}")

main()
