#!/usr/bin/env python3
"""Verify CLI password input never echoes and restores termios on error and termination."""
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import termios
import time

binary=Path(os.environ.get('AGX_CLI_BINARY','.cache/cargo-target/debug/agx')).resolve()
for stop_signal in [None, signal.SIGINT, signal.SIGTERM, signal.SIGHUP]:
    master,slave=pty.openpty()
    before=termios.tcgetattr(slave)
    child=subprocess.Popen([str(binary),'wifi','connect','AGX-QA-Never-Connect'],stdin=slave,stdout=slave,stderr=slave)
    output=bytearray()
    try:
        deadline=time.monotonic()+5
        while b'Wi-Fi password:' not in output:
            assert time.monotonic()<deadline, 'Password prompt missing'
            if select.select([master],[],[],.1)[0]: output.extend(os.read(master,65536))
        assert not termios.tcgetattr(slave)[3] & termios.ECHO
        if stop_signal is None: os.write(master,b'qX7\n')
        else: child.send_signal(stop_signal)
        child.wait(timeout=5)
        while select.select([master],[],[],.05)[0]: output.extend(os.read(master,65536))
        assert child.returncode != 0
        assert termios.tcgetattr(slave)==before, 'Terminal settings were not restored'
        assert b'qX7' not in output, 'Password echoed'
    finally:
        if child.poll() is None: child.kill();child.wait()
        os.close(master);os.close(slave)
print('PASS: CLI password masking and terminal restoration on validation error, SIGINT, SIGTERM and SIGHUP')
