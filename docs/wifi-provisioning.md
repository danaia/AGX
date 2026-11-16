# Wi-Fi provisioning and reboot

AGX 0.3.0 provides Wi-Fi setup and reboot through the Rust TUI. The physical Pi has passed authentication, connectivity, and automatic reconnection after a reboot initiated from the TUI. USB SSH remains available for recovery.

## Use it

Connect through USB SSH, then launch:

```sh
agx tui --color truecolor
```

Press `3` for Network. Scan, select a network, enter or paste its password in the masked field, and choose **Connect & save**. Authentication is saved after successful association. The radio region is initially `US`, as selected for this development Pi.

| Control | Action |
| --- | --- |
| `s` | Scan nearby networks |
| Enter / network click | Open authentication or reconnect a saved network |
| `h` | Enter a hidden network and select its security mode |
| `c` | Change the two-letter radio region |
| `v` | Switch between nearby and saved networks |
| `f` in Saved | Forget the selected profile, with confirmation |
| `d` | Disconnect, with confirmation |
| `i` | Check DNS and HTTPS connectivity |
| `F10` / Reboot button | Open reboot confirmation; Cancel is selected initially |

Forms support keyboard navigation, mouse controls, and bracketed paste. Esc cancels a form; password characters remain in the field rather than triggering global shortcuts. Authentication forms require at least 60 columns and 20 rows. Closing the TUI leaves networking running.

After reboot, reconnect through USB at `10.77.0.2` or through the Wi-Fi address shown in Network. SSH uses keys on both interfaces. Wi-Fi and USB present the same device host key. Wi-Fi addresses come from DHCP and can change.

## Services and credentials

The shared Rust `agx-net` interface connects the TUI to root-owned `agx-netd`. BusyBox init currently supervises the daemon independently of SSH sessions. The daemon manages the existing Linux Broadcom driver, `wpa_supplicant`, DHCP through the Rust `agx-dhcp` hook, DNS, and clock synchronization. Phase 3 will migrate supervision to Rust init.

Credentials are entered through the TUI rather than shell arguments. `/etc/agx` and the runtime directory are root-only; configuration files and the control socket have mode `0600`. WPA2 passphrases become derived PSKs before persistence. WPA3 authentication requires a stored secret in the protected supplicant configuration. Passwords are excluded from status responses and logs. The factory image contains no personal network profiles or Wi-Fi passwords.

Personal WPA2, WPA3, open, and OWE flows are implemented; physical authentication was verified with WPA2. Enterprise authentication, WEP, captive portal login, and static IP configuration are outside this initial flow. The TUI identifies unsupported network security rather than claiming a successful connection.

USB networking is retained while Wi-Fi addresses, routes, and DNS are changed. The connectivity check reaches the OpenAI HTTPS endpoint without an API credential; an HTTP 401 confirms DNS/TLS reachability, not agent authentication.

## Build and development deployment

```sh
make configure
make build
make check-image
```

For an existing development SD installation, use USB SSH and run `make deploy-network`. This builds and installs the TUI, networking service, DHCP hook, firmware, and supporting programs. Deployment preserves saved authentication and restarts the networking daemon, briefly interrupting Wi-Fi. The deployment archive normalizes installed ownership to root. A private BusyBox executable supplies NTP without replacing the running system's BusyBox.

Read-only/diagnostic automation is available through `agx wifi status --json`, `agx wifi scan --json`, and `agx wifi check --json`. Credential entry remains in the TUI.

## Verification — November 15, 2026

Passed on the physical Raspberry Pi 5:

- Broadcom firmware/driver initialization and nearby network scanning.
- User-entered authentication through the masked TUI form, saved WPA2 profile, DHCP address, gateway, and DNS.
- Clock synchronization and HTTPS connectivity to OpenAI (HTTP 401 without an API key).
- Key-authenticated SSH over Wi-Fi, checked against the already trusted USB host key, with USB SSH retained.
- Actual user-triggered TUI reboot, followed by automatic saved Wi-Fi reconnection, clock synchronization, and working SSH over both interfaces.
- Automated SSH terminal checks for scanning, masked paste, invalid password validation, hidden-network cancellation, country editing, saved view, cancellation of disconnect/reboot, small-terminal guards, and private configuration permissions.
- Existing TUI interaction and terminal cleanup regression checks; Rust unit tests, Clippy with warnings denied, ARM64 release compilation, and Buildroot image validation.

Private development evidence is under `.cache/evidence/`, including `wifi-verification.json`, `wifi-pty.json`, and `wifi-image-validation.txt`. The automated PTY record predates the user's successful authentication/reboot; `wifi-verification.json` records the subsequent physical results. Network identities and credentials are not included in this committed record.

Generated `output/images/sdcard.img` SHA-256:

```text
a63f70b501b76b242419abbdf31654f85a51ffb6e111b9c0b641369232176d2b
```

This image passed artifact validation. The installed development SD system passed the reboot test; a fresh flash and cold boot of the regenerated image remain separate release checks. Remaining Phase 2 hardware acceptance and kernel trimming are still pending.
