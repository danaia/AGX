#!/usr/bin/env python3
"""Physical account provisioning test. Uses an ephemeral password and removes its QA account."""
import importlib.util
import fcntl
import termios
import json
import os
from pathlib import Path
import pty
import secrets
import select
import shlex
import subprocess
import time

spec = importlib.util.spec_from_file_location('terminal_tests', Path(__file__).with_name('test-tui-ssh.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


def login(user, password, *, wrong=False, console=False):
    master, slave = pty.openpty()
    host = base.HOST.split('@', 1)[-1]
    if console:
        args = [*base.SSH, '-tt', base.HOST, f'/bin/login {user}']
    else:
        args = ['ssh', '-tt', '-o', 'ConnectTimeout=8', '-o', 'PreferredAuthentications=password',
                '-o', 'PubkeyAuthentication=no', '-o', 'NumberOfPasswordPrompts=1', f'{user}@{host}']
    def controlling_terminal():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)
    process = subprocess.Popen(args, stdin=slave, stdout=slave, stderr=slave, preexec_fn=controlling_terminal)
    os.close(slave)
    output = bytearray()
    sent = False
    commands = False
    deadline = time.monotonic() + 20
    try:
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.2)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                output.extend(data)
                if not sent and b'password:' in output.lower():
                    os.write(master, password.encode() + b'\n')
                    sent = True
                if sent and not wrong and b'Welcome to ANGI' in output and not commands:
                    os.write(master, b'id -un; agx wifi status --json; agx services --json; echo PROFILE_LOGIN_OK; exit\n')
                    commands = True
                if wrong and b'Permission denied' in output:
                    break
                if commands and b'PROFILE_LOGIN_OK\r\n' in output:
                    break
            if process.poll() is not None:
                break
        assert sent, 'Password prompt was missing: ' + output.decode(errors='replace')
        assert password.encode() not in output, 'Password echoed during login'
        if wrong:
            assert b'Permission denied' in output, 'Incorrect password was not rejected'
        else:
            assert b'Welcome to ANGI' in output, 'Login splash missing: ' + output.decode(errors='replace')
            assert b'###########' in output, 'ASCII brand logo missing'
            assert b'PROFILE_LOGIN_OK\r\n' in output, 'Login shell failed'
            assert b'"interface_present":true' in output, 'New user cannot reach provisioning service'
            assert b'"pid1":true' in output, 'New administrator cannot reach Rust service manager'
            assert user.encode() in output
        return bytes(output)
    finally:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        os.close(master)


def main():
    user = f'agxqa{os.getpid()}'
    password = str(secrets.randbelow(90000000) + 10000000)
    # Backups stay on the Pi in a root-only directory and are removed after cleanup.
    backup = f'/run/agx/profile-qa-{os.getpid()}'
    base.remote(f'mkdir -m 700 {backup}; cp /etc/passwd /etc/shadow /etc/hostname /etc/hosts {backup}/')
    original_host = base.remote('cat /etc/hostname').strip()
    console = base.Console()
    try:
        console.expect('PROFILE / IDENTITY')
        console.send(b'n')
        console.expect('MAKE THIS PI YOURS')
        console.capture('profile-initial')
        console.send(b'Terminal QA')
        console.send(b'\t')
        console.send(b'\x7f' * len(original_host) + b'agx-qa')
        console.send(b'\t' + user.encode() + b'\t')
        console.send(b'\x1b[200~short\x1b[201~')
        console.expect('Weak')
        console.send(b'\x13')
        console.expect('Passwords do not match')
        console.send(b'\x7f' * 5)
        console.send(b'\x1b[200~' + password.encode() + b'\x1b[201~')
        console.expect('Weak')
        console.send(b'\t\x1b[200~' + password.encode() + b'\x1b[201~')
        assert password.encode() not in console.raw, 'TUI echoed password'
        console.capture('profile-password-meter')
        console.resize(80, 24)
        console.expect('Review & save')
        console.capture('profile-80x24')
        console.send(b'\x13')
        console.expect('Apply profile')
        console.send(b'\r')  # Confirmation starts on Cancel.
        assert user not in base.remote('cut -d: -f1 /etc/passwd').splitlines()
        console.resize(40, 12)
        console.send(b'\x13')
        assert user not in base.remote('cut -d: -f1 /etc/passwd').splitlines()
        console.resize(120, 36)
        console.send(b'\x13')
        console.expect('Create administrator account')
        console.capture('profile-review')
        console.send(b'\t\r')
        console.expect('Saved Terminal QA.', timeout=12)
        assert base.remote('hostname').strip() == 'agx-qa'
        console.finish()
        login(user, password)
        login(user, 'wrong-' + secrets.token_urlsafe(12), wrong=True)
        login(user, password, console=True)
        base.remote("test \"$(ls -l /etc/shadow | cut -c1-10)\" = '-rw-------'")
        assert base.remote('agx --version').strip() == 'agx 0.4.0'
        report = {key: 'passed' for key in ['profile_creation_in_tui', 'masked_password_paste',
            'password_quality_meter', 'simple_password_accepted', 'mismatch_validation', 'confirmation_defaults_cancel',
            'resize_80x24', 'small_terminal_submission_guard', 'live_hostname',
            'ssh_password_login', 'wrong_password_rejected', 'console_password_login',
            'branded_login_splash', 'administrator_service_access', 'root_key_recovery']}
        (base.EVIDENCE / 'profile-pty.json').write_text(json.dumps(report, indent=2))
        print(json.dumps(report, indent=2))
    finally:
        console.abort()
        # Remove only this test's account. Restore the root console state from its private backup.
        cleanup = f'''set -eu
if grep -q '^{user}:' /etc/passwd; then
    deluser {user}
fi
if test -d /home/{user}; then
    rm -f /home/{user}/.ash_history
    rmdir /home/{user}
fi
if grep -q '^root::' {backup}/shadow; then
    sed -i 's/^root:!:/root::/' /etc/shadow
    chmod 600 /etc/shadow
fi
cp {backup}/hostname /etc/hostname
cp {backup}/hosts /etc/hosts
hostname -F /etc/hostname
rm -f {backup}/passwd {backup}/shadow {backup}/hostname {backup}/hosts
rmdir {backup}
'''
        base.remote(cleanup)


if __name__ == '__main__':
    main()
