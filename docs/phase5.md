# Headless administration

The CLI now uses `clap`, while the CLI and TUI continue to use the same service protocols. This administration work was implemented alongside Phase 3. The agent command and Agent screen still depend on Phase 6; the entire phase is not yet declared complete.

```sh
agx --help
agx status --json
agx profile --json
agx wifi status --json
agx wifi scan --json
agx wifi saved --json
agx wifi select 0
agx wifi country US
agx wifi connect 'My network'
agx wifi connect 'Hidden network' --hidden --security wpa3
agx wifi disconnect --yes
agx wifi forget 0 --yes
agx services --json
agx services restart network --yes --json
agx logs --service network --follow --json
agx reboot --yes
agx shutdown --yes
```

Wi-Fi passwords are entered with terminal echo disabled. Scripts can pass them through standard input using `--password-stdin`; no password argument or environment variable is accepted. Input is bounded, validated and held in zeroizing buffers. Terminal settings are restored after invalid input and SIGINT, SIGTERM or SIGHUP. For open or OWE networks, specify `--security open` or `--security owe`; no password is requested. A connection request is asynchronous: use `agx wifi status --json` to observe association and DHCP results.

Services, network changes and credentials require an AGX administrator account; system sampling remains read-only. Stop/restart, forgotten networks, disconnection, and power commands require confirmation. Interactive prompts default to No, and scripts/JSON calls must explicitly supply `--yes`. Help and invalid-option errors do not perform any service operation. Errors go to stderr with a nonzero exit status; successful JSON output contains no terminal control sequences.

In `agx tui`, press `1` Profile, `2` Overview, `3` Hardware, `4` Network, `5` System, `6` Services, `7` Logs, or `?` Help. Keyboard and mouse work across the workspace. `F9` opens the power-off confirmation and `F10` opens the reboot confirmation; both default to Cancel. Reopening the TUI loads current process state. See [Phase 3](phase3.md) for service controls, journal limits, boot recovery, and deployment.

`python3 scripts/test-cli-password.py` tests password masking and terminal cleanup against the locally built debug CLI. Existing SSH terminal suites and `scripts/test-services-tui-ssh.py` exercise the physical Pi. `tests/requirements-tui.txt` supplies the terminal-emulator dependency.
