#!/usr/bin/env python3
"""Exercise real supervised process recovery, confirmations, journal, and SSH TUI continuity."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time
spec = importlib.util.spec_from_file_location('terminal_tests', Path(__file__).with_name('test-tui-ssh.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

def status():
    return json.loads(m.remote('agx services --json'))

def service(name):
    return next(row for row in status()['services'] if row['name'] == name)

def wait_for(predicate, timeout=15):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        try:
            if predicate(): return
        except subprocess.CalledProcessError as error:
            if error.returncode != 255: raise
        time.sleep(.2)
    raise AssertionError('Timed out waiting for service state')

def main():
    initial=status()
    assert initial['ok']
    if os.environ.get('AGX_EXPECT_PID1') == '1':
        assert initial['pid1']
        assert all(row['state'] == 'running' for row in initial['services'])
        protected = subprocess.run([*m.SSH,m.HOST,'agx services stop ssh --yes --json'],capture_output=True,text=True)
        assert protected.returncode != 0 and not json.loads(protected.stdout)['ok']
        assert service('ssh')['state'] == 'running'
        m.remote('agx services stop cron --yes --json')
        wait_for(lambda: service('cron')['state'] == 'stopped')
        time.sleep(1.2)
        assert service('cron')['state'] == 'stopped'
        m.remote('agx services start cron --json')
        wait_for(lambda: service('cron')['state'] == 'running')
    # Refusal occurs before contacting a service when a script omits confirmation.
    refusal=subprocess.run([*m.SSH,m.HOST,'agx services stop network --json'],capture_output=True,text=True)
    assert refusal.returncode != 0 and 'Confirmation required' in refusal.stderr and not refusal.stdout
    before=service('network')
    assert before['state']=='running'
    m.remote(f"kill -KILL {before['pid']}")
    wait_for(lambda: service('network')['starts'] > before['starts'] and service('network')['state']=='running')
    assert service('network')['pid'] != before['pid']
    # A crash must not leave duplicate WPA or DHCP processes outside the new service tree.
    wait_for(lambda: json.loads(m.remote('agx wifi status --json'))['phase']=='Connected',45)
    assert int(m.remote('pidof wpa_supplicant | wc -w')) == 1
    assert int(m.remote('pidof udhcpc | wc -w')) == 1
    journal=json.loads(m.remote('agx logs --service network --json'))
    assert any('Exited:' in row['message'] for row in journal['logs'])
    assert all(row['service']=='network' for row in journal['logs'])
    target='cron' if initial['pid1'] else 'network'
    c=m.Console()
    try:
        c.expect('LIVE')
        c.send(b'6')
        c.expect('LIVE' if not initial['pid1'] else 'Rust PID 1')
        c.expect('network')
        # Select the actual row, then verify Enter defaults to cancelling restart.
        text=c.read(1)
        row=next(i for i,line in enumerate(c.screen.display) if target in line and 'running' in line)
        c.click(8,row+1)
        current=service(target)
        c.send(b't')
        c.expect('Confirm service operation')
        c.send(b'\r')
        assert service(target)['pid']==current['pid']
        c.send(b't')
        c.expect('Confirm service operation')
        c.send(b'\t\r')
        wait_for(lambda: service(target)['starts']>current['starts'] and service(target)['state']=='running')
        c.read(1.4)
        c.capture('phase3-services')
        # Click stop and cancel using the dialog's mouse target.
        text=c.read()
        row=next(i for i,line in enumerate(c.screen.display) if '[x Stop]' in line)
        col=c.screen.display[row].index('[x Stop]')+1
        c.click(col,row+1)
        c.expect('Confirm service operation')
        cancel_row=next(i for i,line in enumerate(c.screen.display) if 'Cancel' in line)
        cancel_col=c.screen.display[cancel_row].index('Cancel')+1
        c.click(cancel_col,cancel_row+1)
        assert service(target)['state']=='running'
        c.send(b'7')
        c.expect('Lifecycle journal')
        c.expect('Started PID')
        c.send(b'f')
        c.expect('Filter: init')
        c.send(b'/manager\r')
        c.expect('Search: manager')
        c.expect('Rust service manager started')
        c.send(b'\x1b[A\x1b[B')
        c.capture('phase3-logs')
        c.resize(80,24)
        c.expect('Search: manager')
        c.resize(40,12)
        c.resize(120,36)
        c.finish()
    finally:
        c.abort()
    stable=service(target)['pid']
    c=m.Console('--ascii --no-color --no-mouse')
    try:
        c.expect('LIVE')
        c.send(b'6')
        c.expect(str(stable))
        c.finish()
    finally:
        c.abort()
    assert service(target)['pid']==stable
    report={'rust_pid1':initial['pid1'],'crash_restart':'passed','child_cleanup':'passed',
            'script_confirmation':'passed','keyboard_and_mouse_confirmation':'passed',
            'live_logs_filter_search_resize':'passed','supervision_after_tui_exit_and_reopen':'passed'}
    (m.EVIDENCE/'phase3-services.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))

if __name__=='__main__': main()
