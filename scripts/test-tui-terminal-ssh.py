#!/usr/bin/env python3
"""Exercise TUI shell job control, tmux and terminal restoration on the Pi; requires pyte.

Set AGX_SSH_HOST, optional AGX_SSH_HOST_KEY_ALIAS, AGX_TEST_USER, and
AGX_REMOTE_BINARY (defaults to installed /usr/bin/agx). Uses an isolated tmux server.
"""
import codecs
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import shlex
import struct
import subprocess
import termios
import time

import pyte

ssh = ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-i', os.environ.get('AGX_SSH_KEY', str(Path.home() / '.ssh/id_ed25519'))]
if (alias := os.environ.get('AGX_SSH_HOST_KEY_ALIAS')):
    ssh += ['-o', 'HostKeyAlias=' + alias, '-o', 'StrictHostKeyChecking=yes']
host = os.environ.get('AGX_SSH_HOST', 'root@10.77.0.2')
user = os.environ.get('AGX_TEST_USER', 'dana')
binary = os.environ.get('AGX_REMOTE_BINARY', '/usr/bin/agx')
base = f'/var/lib/agx-data/agx-tui-terminal-qa-{os.getpid()}'

def remote(command):
    return subprocess.check_output(ssh + [host, command], text=True, timeout=20)
remote('mountpoint -q /var/lib/agx-data && mkdir -p ' + shlex.quote(base + '/tmux') + ' && chown -R ' + shlex.quote(user) + ' ' + shlex.quote(base) + ' && chmod 700 ' + shlex.quote(base) + ' ' + shlex.quote(base + '/tmux'))
command = shlex.join(['env', 'TERM=xterm-256color', 'HISTFILE=/dev/null', 'TMUX_TMPDIR=' + base + '/tmux', binary, 'tui', '--ascii', '--no-color', '--no-mouse'])
inner = 'before=$(stty -g); ' + command + '; result=$?; after=$(stty -g); if [ "$before" = "$after" ]; then restored=yes; else restored=no; fi; printf "\\nTTY_RESTORED=%s\\n" "$restored"; exit "$result"'
fd, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 36, 120, 0, 0))
process = subprocess.Popen(ssh + ['-tt', host, 'su - ' + shlex.quote(user) + ' -c ' + shlex.quote(inner)], stdin=slave, stdout=slave, stderr=slave)
os.close(slave)
log = bytearray()
screen = pyte.Screen(120, 36)
screen.write_process_input = lambda value: os.write(fd, value.encode())
report = screen.report_device_status

def report_status(mode, **kwargs):
    if not kwargs.get('private'):
        report(mode)
    elif mode == 6:
        os.write(fd, b'\x1b[?1;1R')
screen.report_device_status = report_status
screen.report_device_attributes = lambda *a, **kwargs: None
stream = pyte.Stream(screen)
decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')

def until(marker, timeout=8):
    start = time.monotonic()
    data = bytearray()
    while time.monotonic() - start < timeout:
        if select.select([fd], [], [], 0.1)[0]:
            chunk = os.read(fd, 65536)
            log.extend(chunk)
            data.extend(chunk)
            if b'\x1b[?1049h' in chunk or b'\x1b[?1049l' in chunk:
                screen.reset()
            stream.feed(decoder.decode(chunk))
            if marker in data or (not marker.startswith((b'\r', b'\x1b')) and marker.decode() in '\n'.join(screen.display)):
                return bytes(data)
    raise AssertionError(('Missing', marker, bytes(data[-1200:])))

def send(s):
    os.write(fd, s if isinstance(s, bytes) else s.encode())
try:
    until(b'ANGI')
    time.sleep(0.3)
    send(b'8')
    until(b'YOUR SHELL')
    send(b'\x1bOS')
    until(b'\x1b[?1049l')
    time.sleep(0.3)
    send("printf '\\n__SHELL_READY__\\n'\r")
    until(b'\r\n__SHELL_READY__\r\n')
    send(b'sleep 30\r')
    time.sleep(0.25)
    send(b'\x03')
    time.sleep(0.2)
    send("printf '\\n__INTERRUPT_OK__\\n'\r")
    until(b'\r\n__INTERRUPT_OK__\r\n')
    send(b'sleep 30\r')
    time.sleep(0.25)
    send(b'\x1a')
    until(b'Stopped')
    send(b'fg\r')
    time.sleep(0.2)
    send(b'\x03')
    time.sleep(0.2)
    send(b'\x15exit\r')
    until(b'YOUR SHELL')
    send(b'\r')
    until(b'\x1b[?1049l')
    time.sleep(0.2)
    send("printf '\\n__REENTERED__\\n'\r")
    until(b'\r\n__REENTERED__\r\n')
    send(b'stty -ixon\r')
    time.sleep(0.1)
    send(b'\x15exit\r')
    until(b'YOUR SHELL')
    send(b't')
    until(b'\x1b[?1049l')
    time.sleep(0.4)
    send("printf '\\n__TMUX_RUNNING__\\n'\r")
    until(b'__TMUX_RUNNING__')
    time.sleep(0.3)
    send(b'\x02d')
    until(b'YOUR SHELL')
    send(b't')
    until(b'\x1b[?1049l')
    time.sleep(0.4)
    send(b'\x15exit\r')
    until(b'YOUR SHELL')
    if os.environ.get('AGX_TEST_OPENCODE') == '1':
        send(b'o')
        until(b'\x1b[?1049l')
        until(b'Ask anything', timeout=20)
        screen.resize(28, 100)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 28, 100, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        time.sleep(.4)
        send(b'\x03')
        until(b'YOUR SHELL')
    screen.resize(24, 80)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
    os.kill(process.pid, signal.SIGWINCH)
    time.sleep(0.2)
    send(b'?')
    until(b'HEADLESS BY DESIGN')
    send(b'q')
    until(b'TTY_RESTORED=yes')
    assert process.wait(timeout=10) == 0
    print('PTY checks passed: shell handoff, Ctrl+C, Ctrl+Z/fg, return, tmux detach/reattach, optional OpenCode, resize, help, quit.')
finally:
    Path('.cache/tui-shell-ssh.ansi').write_bytes(log)
    Path('.cache/tui-shell-ssh-screen.txt').write_text('\n'.join(screen.display))
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    try:
        cleanup = shlex.join(['env', 'TMUX_TMPDIR=' + base + '/tmux', 'tmux', 'kill-server'])
        remote('su - ' + shlex.quote(user) + ' -c ' + shlex.quote(cleanup) + ' >/dev/null 2>&1 || true')
        remote('rm -rf ' + shlex.quote(base))
    finally:
        os.close(fd)
