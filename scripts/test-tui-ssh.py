#!/usr/bin/env python3
"""Exercise the real Pi TUI through an SSH PTY. Requires pyte on the workstation."""
import codecs
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import shlex
import signal
import struct
import subprocess
import termios
import time

import pyte

HOST = os.environ.get('AGX_SSH_HOST', 'root@10.77.0.2')
KEY = os.environ.get('AGX_SSH_KEY', str(Path.home() / '.ssh/id_ed25519'))
SSH = ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-i', KEY]
EVIDENCE = Path('.cache/evidence')
EVIDENCE.mkdir(parents=True, exist_ok=True)


def remote(command):
    return subprocess.check_output([*SSH, HOST, command], text=True)


class Console:
    def __init__(self, options=''):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 36, 120, 0, 0))
        self.screen = pyte.Screen(120, 36)
        # A real terminal answers cursor/device queries during backend setup.
        self.screen.write_process_input = lambda value: os.write(self.master, value.encode())
        self.stream = pyte.Stream(self.screen)
        self.raw = bytearray()
        self.decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
        self.pidfile = f'/run/agx-tui-qa-{os.getpid()}.pid'
        command = (
            'export TERM=xterm-256color COLORTERM=truecolor; '
            'before=$(stty -g); '
            f'agx tui {options} < /dev/tty & app_pid=$!; echo "$app_pid" > {self.pidfile}; '
            'wait "$app_pid"; app_exit=$?; after=$(stty -g); '
            'if [ "$before" = "$after" ]; then restored=yes; else restored=no; fi; '
            f'rm -f {self.pidfile}; '
            'printf "\\nTTY_RESTORED=%s APP_EXIT=%s\\n" "$restored" "$app_exit"'
        )
        self.process = subprocess.Popen([*SSH, '-tt', HOST, command], stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        self.closed = False

    def read(self, duration=0.4):
        end = time.monotonic() + duration
        while time.monotonic() < end:
            if select.select([self.master], [], [], max(0, end-time.monotonic()))[0]:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    break
                if not data:
                    break
                self.raw.extend(data)
                self.stream.feed(self.decoder.decode(data))
        return '\n'.join(self.screen.display)

    def expect(self, phrase, timeout=8):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            text = self.read(0.2)
            if phrase in text:
                return text
            if self.process.poll() is not None:
                break
        raise AssertionError(f'TUI did not display {phrase!r}:\n' + '\n'.join(self.screen.display))

    def send(self, value):
        os.write(self.master, value)
        return self.read()

    def click(self, column, row):
        return self.send(f'\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m'.encode())

    def resize(self, width, height):
        self.screen.resize(height, width)
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack('HHHH', height, width, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)
        return self.read(0.8)

    def capture(self, name):
        (EVIDENCE / f'{name}.txt').write_text('\n'.join(self.screen.display))
        (EVIDENCE / f'{name}.ansi').write_bytes(self.raw)
        cells = []
        for row in range(self.screen.lines):
            cells.append([{'text': self.screen.buffer[row][column].data, 'fg': self.screen.buffer[row][column].fg,
                           'bg': self.screen.buffer[row][column].bg, 'bold': self.screen.buffer[row][column].bold}
                          for column in range(self.screen.columns)])
        (EVIDENCE / f'{name}.cells.json').write_text(json.dumps(cells))

    def finish(self, key=b'q', terminate=False):
        if terminate:
            remote(f'kill -TERM "$(cat {shlex.quote(self.pidfile)})"')
        else:
            self.send(key)
        self.process.wait(timeout=10)
        self.read(0.1)
        text = self.raw.decode(errors='replace')
        assert 'TTY_RESTORED=yes APP_EXIT=0' in text, text[-2000:]
        assert b'\x1b[?1006l' in self.raw, 'Mouse capture not disabled on exit'
        assert b'\x1b[?1049l' in self.raw, 'Alternate screen not left on exit'
        self.closed = True
        os.close(self.master)

    def abort(self):
        if not self.closed:
            try:
                remote(f'test ! -f {self.pidfile} || kill -TERM "$(cat {self.pidfile})"')
            finally:
                self.process.terminate()
                self.process.wait(timeout=10)
                os.close(self.master)
                self.closed = True


def main():
    status = json.loads(remote('agx status --json'))
    assert 'Raspberry Pi 5' in status['board']
    assert status['cpu_count'] == 4
    assert status['memory']['total_bytes'] > 15 * 1024**3
    assert any(net['name'] == 'usb0' and '10.77.0.2' in net['ipv4'] for net in status['networks'])
    assert any(row['name'] == 'mmcblk0' for row in status['hardware'])
    (EVIDENCE / 'phase2-status.json').write_text(json.dumps(status, indent=2))
    console = Console()
    measurements = {}
    try:
        console.expect('LIVE')
        console.send(b'2')
        console.expect('THIS MACHINE')
        console.read(1.3)
        assert 'Sampling CPU' not in '\n'.join(console.screen.display), 'CPU sampler did not refresh'
        assert '\ufffd' not in '\n'.join(console.screen.display), 'Invalid UTF-8 in terminal output'
        console.capture('phase2-overview')
        app_pid = int(remote(f'cat {console.pidfile}').strip())
        def counters():
            sample = remote(f'cat /proc/{app_pid}/stat; grep VmRSS /proc/{app_pid}/status; head -1 /proc/stat').splitlines()
            fields = sample[0].split()
            return int(fields[13])+int(fields[14]), sum(map(int, sample[2].split()[1:9])), int(sample[1].split()[1])*1024
        before = counters()
        console.read(2)
        after = counters()
        measurements['rss_bytes'] = after[2]
        measurements['idle_cpu_percent_one_core'] = round((after[0]-before[0]) / max(1, after[1]-before[1]) * status['cpu_count'] * 100, 2)
        assert b'\x1b[?1006h' in console.raw, 'Mouse capture not enabled'
        console.click(38, 5)
        console.expect('HARDWARE INVENTORY')
        console.click(22, 11)
        console.expect('Selected device')
        selected = status['hardware'][min(2, len(status['hardware'])-1)]
        assert selected['name'] in console.screen.display[32], console.screen.display[32]
        console.send(b'\x1b[<65;22;11M')
        selected = status['hardware'][min(5, len(status['hardware'])-1)]
        assert selected['name'] in console.screen.display[32], console.screen.display[32]
        console.capture('phase2-hardware')
        console.send(b'?')
        console.expect('HEADLESS BY DESIGN')
        console.send(b'2')
        console.expect('THIS MACHINE')
        console.resize(80, 24)
        console.expect('THIS MACHINE')
        console.capture('phase2-80x24')
        console.resize(40, 12)
        console.expect('compact')
        console.capture('phase2-40x12')
        console.resize(20, 6)
        console.expect('Resize to at least')
        console.resize(120, 36)
        console.expect('THIS MACHINE')
        console.finish()
    finally:
        console.abort()
    for options, action in [('--ascii --no-color --no-mouse', 'ctrl-c'), ('', 'sigterm')]:
        console = Console(options)
        try:
            console.expect('LIVE')
            if '--ascii' in options:
                text = '\n'.join(console.screen.display)
                assert text.isascii(), text
                assert b'\x1b[?1006h' not in console.raw
                assert all(cell.fg == 'default' and cell.bg == 'default' for row in console.screen.buffer.values() for cell in row.values())
            console.finish(key=b'\x03', terminate=action == 'sigterm')
        finally:
            console.abort()
    console = Console()
    try:
        console.expect('LIVE')
        app_pid = int(remote(f'cat {console.pidfile}').strip())
        console.process.kill()
        console.process.wait(timeout=10)
        os.close(console.master)
        console.closed = True
        time.sleep(1)
        result = remote(f'if test -r /proc/{app_pid}/comm; then cat /proc/{app_pid}/comm; else echo exited; fi').strip()
        if result == 'agx':
            remote(f'kill -KILL {app_pid}; rm -f {console.pidfile}')
            raise AssertionError('TUI survived a lost SSH connection')
        remote(f'rm -f {console.pidfile}')
    finally:
        console.abort()
    noninteractive = subprocess.run([*SSH, HOST, 'agx tui'], capture_output=True, text=True)
    assert noninteractive.returncode != 0 and not noninteractive.stdout
    assert 'interactive terminal' in noninteractive.stderr
    report = {'status_json': 'passed', 'mouse_tabs_rows_scroll': 'passed', 'keyboard_navigation': 'passed',
              'resize_120_80_40_20_columns': 'passed', 'ascii_no_color_no_mouse': 'passed',
              'terminal_restore_q_ctrl_c_sigterm': 'passed', 'noninteractive_refusal': 'passed', 'ssh_disconnect_exit': 'passed', 'resource_sample': measurements}
    (EVIDENCE / 'phase2-pty.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
