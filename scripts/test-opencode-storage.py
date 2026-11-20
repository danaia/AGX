#!/usr/bin/env python3
"""Exercise the launcher with an isolated store and fake engine; no credentials or network."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="angi-opencode-") as temporary:
    base = Path(temporary)
    users = base / "users"
    users.mkdir(mode=0o1777)
    tools = base / "bin"
    tools.mkdir()
    mountpoint = tools / "mountpoint"
    mountpoint.write_text("#!/bin/sh\nexit 0\n")
    mountpoint.chmod(0o755)
    engine = tools / "engine"
    engine.write_text('#!/bin/bash\nprintf "%s\\n" "$PWD" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$OPENCODE_DISABLE_AUTOUPDATE" "$@"\n')
    engine.chmod(0o755)
    board = base / "board"
    launcher = base / "opencode"
    launcher.write_text((ROOT / "apps/opencode/launch.sh").read_text()
                        .replace("/usr/share/agx/board", str(board))
                        .replace("/var/lib/agx-data/agx-opencode-users", str(users))
                        .replace("/usr/libexec/opencode/opencode", str(engine)))
    launcher.chmod(0o755)
    env = {**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"]}
    work = base / "project with spaces"
    work.mkdir()
    result = subprocess.run([str(launcher), "--version", "a b"], cwd=work, env=env, capture_output=True, text=True, check=True)
    store = users / str(os.getuid())
    assert result.stdout.splitlines() == [str(work), str(store / "data"), str(store / "config"), "true", "--version", "a b"]
    assert store.stat().st_mode & 0o777 == 0o700
    assert (store / "cache").stat().st_mode & 0o777 == 0o700
    # A symlink to an owned directory must still be refused, without modifying it.
    import shutil
    shutil.rmtree(store)
    victim = base / "victim"
    victim.mkdir(mode=0o755)
    victim.chmod(0o755)
    store.symlink_to(victim, target_is_directory=True)
    result = subprocess.run([str(launcher)], cwd=work, env=env, capture_output=True, text=True)
    assert result.returncode != 0
    assert not list(victim.iterdir())
    assert victim.stat().st_mode & 0o777 == 0o755
    mountpoint.write_text("#!/bin/sh\nexit 1\n")
    result = subprocess.run([str(launcher)], cwd=work, env=env, capture_output=True, text=True)
    assert result.returncode != 0 and "NVMe" in result.stderr
    board.write_text("zero2w\n")
    result = subprocess.run([str(launcher)], cwd=work, env=env, capture_output=True, text=True)
    assert result.returncode != 0 and not list(victim.iterdir())
    store.unlink()
    result = subprocess.run([str(launcher), "--version"], cwd=work, env=env, capture_output=True, text=True, check=True)
    assert result.stdout.splitlines() == [str(work), str(store / "data"), str(store / "config"), "true", "--version"]
    assert store.stat().st_mode & 0o777 == 0o700
print("OpenCode launcher checks passed for NVMe and SD storage.")
