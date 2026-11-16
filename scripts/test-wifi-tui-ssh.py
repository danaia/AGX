#!/usr/bin/env python3
"""Exercise provisioning dialogs on the physical Pi without submitting credentials or rebooting."""
import importlib.util
import json
import time
from pathlib import Path

spec=importlib.util.spec_from_file_location('terminal_tests',Path(__file__).with_name('test-tui-ssh.py'))
base=importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


def main():
    status=json.loads(base.remote('agx wifi status --json'))
    assert status['country']=='US' and status['interface_present']
    console=base.Console()
    try:
        console.expect('LIVE')
        console.send(b'3')
        console.expect('NETWORK PROVISIONING')
        console.send(b's')
        deadline=time.monotonic()+15
        while True:
            console.read(0.5)
            current=json.loads(base.remote('agx wifi status --json'))
            if current['access_points'] and not current['scanning']:
                break
            assert time.monotonic()<deadline, 'Wi-Fi scan did not finish'
        assert current['access_points'], 'Physical Wi-Fi scan found no networks'
        console.capture('wifi-provisioning')
        console.send(b'\r')
        console.expect('Wi-Fi authentication')
        console.send(b'q')
        console.send(b'\r')
        console.expect('printable ASCII')
        assert not json.loads(base.remote('agx wifi status --json'))['busy'], 'Invalid password was submitted'
        console.send(b'\x7f')
        secret=b'q-private-test-only'
        console.send(b"\x1b[200~"+secret+b"\x1b[201~")
        assert secret not in console.raw, 'Password leaked into terminal output'
        console.capture('wifi-authentication')
        console.send(b'\x1b')
        console.expect('NETWORK PROVISIONING')
        console.send(b'h')
        console.expect('Wi-Fi authentication')
        console.send(b'AGX-Test-Never-Connect')
        console.send(b'\t\t')
        console.send(b"\x1b[200~"+secret+b"\x1b[201~")
        assert secret not in console.raw
        console.send(b'\x1b')
        console.send(b'c')
        console.expect('Two-letter country code')
        console.send(b'\x7f\x7fus')
        console.send(b'\r')
        console.expect('Radio region saved')
        console.send(b'v')
        console.expect('SAVED NETWORKS')
        console.send(b'v')
        console.send(b'd')
        console.expect('Disconnect Wi-Fi')
        console.send(b'\r')
        console.expect('NETWORK PROVISIONING')
        console.click(113,35)
        console.expect('Reboot Pi')
        console.capture('wifi-reboot-confirmation')
        console.send(b'\r')
        console.expect('NETWORK PROVISIONING')
        console.resize(40,12)
        console.send(b'h')
        console.expect('Provisioning dialog')
        console.send(b'\r')
        console.send(b'\x1b')
        console.resize(120,36)
        console.expect('NETWORK PROVISIONING')
        console.finish()
    finally:
        console.abort()
    final=json.loads(base.remote('agx wifi status --json'))
    assert not any(n['ssid']=='AGX-Test-Never-Connect' for n in final['saved'])
    metadata=base.remote('ls -ld /etc/agx /etc/agx/network.json /etc/agx/wpa_supplicant.conf /run/agx/network.sock').splitlines()
    assert [line.split()[0] for line in metadata]==['drwx------','-rw-------','-rw-------','srw-------'], metadata
    assert all(line.split()[2:4]==['root','root'] for line in metadata), metadata
    report={'physical_scan':'passed','masked_password':'passed','invalid_password_stays_local':'passed',
            'hidden_network_form_cancel':'passed','country_edit_save':'passed','disconnect_cancel':'passed',
            'reboot_defaults_cancel':'passed','small_terminal_guard':'passed','private_permissions':'passed',
            'actual_authentication':'awaiting user credentials in TUI','actual_reboot':'not performed'}
    (base.EVIDENCE/'wifi-pty.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))


if __name__=='__main__':
    main()
