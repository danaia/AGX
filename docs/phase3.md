# Phase 3 — Rust startup, services, and lifecycle logs

AGX now has a Rust `agx-init` service manager, shared service protocol, Services and Logs screens, and CLI administration commands. Update downloading, installation, and card migration remain on hold.

## Operating the Pi

Run `agx tui` over interactive SSH. Press `6` for Services, select a service with arrows or the mouse, and use `s` to start, `x` to stop, or `t` to restart it. Stop and restart show a confirmation that defaults to Cancel. Network operations briefly interrupt Wi-Fi; USB SSH remains the recovery path. SSH, logging, and recovery console services are protected from these controls.

Press `7` for Logs. The live journal records service starts, exits, spawn failures, and administrative operations. Use `f` to cycle the service filter, `/` to enter a message search, arrows or the mouse wheel to scroll, and End to return to the latest events. The journal holds the latest 512 lifecycle events from this boot in memory; it is separate from the system's rotated syslog. Child stdout/stderr are not copied into the journal, keeping authentication output and credentials out of these views.

F9 opens power-off confirmation; F10 opens reboot confirmation. Both default to Cancel. The new commands use the same Rust service operations as the TUI:

```sh
agx services
agx services --json
agx services restart network --yes
agx services stop cron --yes
agx services start cron
agx logs --service network
agx logs --follow --json
agx reboot --yes
agx shutdown --yes
```

Interactive stop/restart and power commands prompt unless `--yes` is supplied. Noninteractive and JSON callers must explicitly supply `--yes`. JSON output contains no terminal styling. `logs --follow --json` emits one JSON object per poll; sequence numbers and truncation flags allow consumers to identify journal expiry and daemon restarts. A service marked `running` has a live process; application readiness, including Wi-Fi association, is reported by the relevant service's own interface.

## Startup and recovery

The kernel command line selects `init=/usr/sbin/agx-init`. Rust PID 1 mounts the minimal filesystems, sets the hostname, and runs the existing entropy, sysctl, module, and USB network initialization scripts. It supervises foreground syslog, kernel logging, Dropbear SSH, HDMI and serial gettys, cron, and the existing network/profile service. The existing Linux kernel and SD partition layout are retained.

Child exits trigger bounded exponential restart delays, from one to 32 seconds. PID/start-time identities track service descendants, including children in separate process groups. A crashed service's remaining children are killed before replacement, and adopted children are reaped. Requested stops remain stopped; restart is asynchronous and waits for the old process to exit. Graceful termination allows five seconds before forced cleanup. Power operations reply before closing SSH, stop services, flush writes, unmount/remount filesystems read-only, then request the kernel reboot or poweroff operation.

If bootstrap fails, Rust executes the retained `/bin/busybox init` as PID 1. BusyBox's original mounts, init scripts and console entries remain; its inittab starts `agx-init --supervise` for the AGX network service. A missing SSH listener after the first 15 seconds also triggers recovery startup. A bootstrap failure is recorded in `/run/agx-init-boot-error`. Closing, crashing, or disconnecting the TUI does not stop the supervisor.

The root-owned `/run/agx/init.sock` is mode `0660` for `agx-admin`; other users cannot issue commands or read its journal. Inputs, replies, timeouts, and journal size are bounded. Service definitions are compiled in; callers can choose a known service operation, never an arbitrary root command. SSH and console services are protected from stop/restart requests.

## Development deployment

```sh
make init-build
make deploy-init
./scripts/enable-rust-init.sh
```

`deploy-init` first transitions the current BusyBox installation to the Rust supervisor without rebooting. It preserves the original inittab and network binary in `/root/agx-init-backup`. The enable script checks the executables and adds the Rust init argument to the boot partition, preserving the original command line in that backup directory. A subsequent confirmed reboot exercises Rust PID 1. These scripts do not rewrite system images, repartition storage, or replace accounts and saved networks.

For recovery through a root SSH shell, restore `/root/agx-init-backup/cmdline.txt` to `cmdline.txt` on the FAT boot partition and reboot. If SSH is unavailable, restore that file using the card reader. To retain Rust supervision beneath BusyBox, simply remove the `init=/usr/sbin/agx-init` argument. Full removal additionally restores the original inittab and network executable from the backup. New Buildroot images select Rust init and retain BusyBox as fallback.

## Acceptance evidence

Run the workspace tests and Clippy, build ARM64 binaries and the complete Buildroot image, and run `make check-image`. Physical SSH terminal coverage is in `scripts/test-services-tui-ssh.py`; set `AGX_EXPECT_PID1=1` to require Rust PID 1. The existing TUI and Wi-Fi provisioning suites continue to test the earlier screens and terminal cleanup. Generated artifacts and captures are stored under `.cache/evidence` and are not published as source.

Headless acceptance does not imply a physical HDMI/UART login was tested. Those recovery paths remain separate hardware checks. The radio firmware warnings and untested wired Ethernet connection remain documented in the hardware audit.

On November 15, 2026, the development Pi rebooted into Rust PID 1 and subsequently rebooted through the Rust power path. All seven service processes were observed running. The physical SSH suites passed for crash restart and WPA/DHCP child cleanup; persistent stop/start and confirmed restart; protected SSH controls and refusal of unprivileged socket access; keyboard/mouse confirmations; filtered/searchable logs; narrow layouts and terminal restoration; TUI exit/reopen and abrupt SSH disconnection; saved Wi-Fi reconnection; and named-administrator password login with access to both service sockets. F9 power-off confirmation was cancelled during testing; an actual physical power-off cycle was not performed. Existing accounts, password hashes, hostname, Wi-Fi files, SSH host keys and the root recovery key matched their pre-transition fingerprints after testing.

Workspace tests, Clippy with warnings denied, local password masking/interrupt tests, ARM64 compilation, the complete Buildroot build, and image validation passed. The complete fresh image was validated but was not flashed; the running Pi received the binaries and boot selector in place. A brief sample measured Rust PID 1 at 1,830,912 bytes RSS and 3.85% of one CPU core; the TUI regression sample measured 3,612,672 bytes RSS and 0.96% of one core. These short observations are not release benchmarks.

The HDMI console clears the display before its minimal AGX sign-in prompt. Sign in with an existing administrator account to reach the shell; use `agx --help` for commands or `agx tui` for the dashboard. Kernel messages go to the serial console and remain in system logs.
