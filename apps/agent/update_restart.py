"""Activation impact of ANGI's supported update paths, separate from pending restarts."""
import refresh
import releases


def impact(key, owner):
    if key == 'display-access':
        return ('computer', 'Reboot after update',
                'The device-access service starts with the system. A reboot reloads it and restores access to input and display devices.')
    if owner in {'kernel', 'base', 'busybox', 'openssl'}:
        return ('computer', 'Reboot after update',
                'This component is part of the system image. A computer reboot is required to use the new version. '
                'Save your work first: a reboot closes applications, terminal sessions and remote connections. '
                'Installing a kernel does not change the kernel that is currently running.')
    if owner == 'desktop':
        return ('desktop', 'Desktop restart',
                'Restart the desktop to load the installed release. This closes ANGI terminal and browser sessions, '
                'but does not reboot the computer. Save your work first. Use Settings → Refresh desktop when you are ready.')
    if owner == 'web':
        return ('application', 'Browser restart',
                'Close and reopen Web to load the updated browser engine. Open tabs and browser sessions will close. '
                'A computer reboot is not required.')
    if owner == 'bluetooth':
        return ('service', 'Bluetooth restart',
                'Restart the Bluetooth service to use the update. Bluetooth headphones, keyboards and mice may disconnect. '
                'A computer reboot is not normally required.')
    if owner == 'tailscale':
        return ('service', 'Tailscale restart',
                'Restart Tailscale to use the update. Tailnet connections and the LM Studio relay may disconnect temporarily. '
                'A computer reboot is not normally required.')
    if owner == 'ssh':
        return ('service', 'SSH service restart',
                'Restart the SSH server to use the update. Remote administration connections may disconnect; '
                'keep another recovery connection available. A computer reboot is not normally required.')
    if owner == 'wpa':
        return ('service', 'Network restart',
                'Restart the Wi-Fi service to use the update. Wi-Fi and remote connections using it may disconnect temporarily. '
                'A computer reboot is not normally required.')
    if owner == 'tmux':
        return ('application', 'tmux restart',
                'Restart existing tmux servers to load the new version. This can close tmux sessions and the commands running in them. '
                'Save your work first. A computer reboot is not required.')
    if owner == 'terminal':
        return ('application', 'New SSH sessions',
                'New SSH connections use the updated client. Existing SSH processes keep their current version until they exit. '
                'A desktop or computer restart is not required.')
    if owner == 'agent':
        return ('application', 'Agent restart',
                'New agent sessions use the updated runtime. Running agent sessions need to be closed and reopened to use it. '
                'A computer reboot is not required.')
    return ('review', 'Restart review needed',
            'The installer must identify which applications or services need restarting before it can safely activate this update.')


def decorate(rows, owners):
    current = releases.installed()
    # Only claim a pending restart when this account actually runs an older desktop.
    running = refresh.desktops()
    desktop_pending = bool(running) and any(name != current for name in running.values())
    for row in rows:
        owner = owners.get(row['key'], row['key'])
        kind, label, detail = impact(row['key'], owner)
        row.update(restart_kind=kind, restart_label=label, restart_detail=detail,
                   restart_pending=row['key'] == 'desktop' and desktop_pending)
        if row['restart_pending']:
            row['restart_label'] = 'Restart pending'
        row['install_note'] = ("Build and install from the guarded local source. Your session stays open during installation; "
                               "activation is a separate step. The previous release is retained for rollback."
                               if row.get('action') else
                               "Installation from this panel is not available for this component yet. "
                               "The release check shows an upstream version, not an installable ANGI package. "
                               "Nothing will be installed or restarted from this review.")
    return rows
