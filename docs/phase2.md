# Phase 2 — Rust TUI foundation

The first Phase 2 deliverable is implemented and running on the physical Pi 5. The remaining kernel and hardware work in [the plan](../plans/plan1.md) is still pending. Wi-Fi provisioning and reboot have since been added and verified; see the [Wi-Fi guide and acceptance record](wifi-provisioning.md).

## Use it

From the Pi's existing SSH shell:

```sh
agx tui
```

From the workstation:

```sh
ssh -t -i ~/.ssh/id_ed25519 root@10.77.0.2 agx tui
```

Overview shows live CPU usage, memory, root filesystem space, board/kernel identity, uptime, load, and network interfaces. Hardware lists detected block devices, interfaces, and loaded modules. Selecting a row shows its details. Help explains all controls.

- Click the tabs or device rows; use the mouse wheel to scroll/select.
- Press `1`, `2`, `3`, or `?` for Overview, Hardware, Network, or Help. `F10` opens the reboot confirmation.
- Tab/Shift-Tab and Left/Right change screens. Up/Down or `j`/`k` scroll/select; PageUp/PageDown move by a page, and Home/End select hardware entries.
- `r` requests a new sample. `q`, Esc, or Ctrl-C returns to the shell.
- Smaller terminals get a compact overview; below 40 columns or 12 rows, a resize notice replaces the dashboard.

Color is selected from `COLORTERM` and `TERM`, with `NO_COLOR` respected in auto mode. If an SSH client does not forward its full-color capability, use `agx tui --color truecolor`. Explicit modes: `auto`, `truecolor`, `256`, `16`, and `never`.

```sh
agx tui --ascii --no-color --no-mouse
agx status
agx status --json
```

Icons have adjacent text labels. ASCII mode replaces decorative Unicode, including borders and gauges. Fonts come from the workstation terminal. The TUI starts explicitly in interactive terminals; ordinary SSH commands and JSON output remain usable for automation.

## Implementation

- `agx-core`: Linux data collection and shared serializable models. CPU usage is sampled from counter deltas; memory uses `MemAvailable`, root space uses `statvfs`, and inventory comes from `/proc` and `/sys`. IPv4 addresses use `getifaddrs`.
- `agx-tui`: Ratatui 0.30.2 with Crossterm 0.29.0, a background sampler, and keyboard/mouse event handling. Last successful data remains visible with a stale/error indicator if collection fails. Disconnection closes the TUI; reopening starts a fresh collection.
- `agx-cli`: the initial `agx tui`, `agx status [--json]`, help, and version commands.
- `Cargo.lock` locks dependencies. Rust 1.97.1 matches the pinned Buildroot host Rust toolchain. Buildroot vendors the workspace dependencies before building offline with its native Cargo package infrastructure.
- Cleanup restores terminal modes, mouse reporting, cursor visibility, and the alternate screen. A bounded TTY input backend plus hangup detection prevents an abandoned background TUI from spinning after SSH disconnect.

## Build and deploy

`make configure`, `make build`, and `make check-image` include the Rust console in the generated image. Buildroot supplies its host Rust/Cargo toolchain automatically. The local source package is staged under `.cache/agx-package`; `agx-rebuild` picks up workspace changes on subsequent builds.

For workstation development, install Rust through rustup; `rust-toolchain.toml` selects the toolchain, formatting/lint components, and ARM64 standard library. The project wrapper also supports the locally cached Rust installation used during bring-up.

```sh
make tui-test
./scripts/cargo.sh clippy --workspace --all-targets --locked -- -D warnings
make tui-build
make deploy-tui
```

Cross-compilation uses the existing Bootlin compiler under `output/host/bin/`, so prepare the Buildroot toolchain through `make build` first on a new workstation. `make deploy-tui` copies the binary over legacy SCP, checks it runs, and replaces `/usr/bin/agx` while retaining the previous binary as `/usr/bin/agx.prev`. Override `AGX_SSH_HOST` and `AGX_SSH_KEY` for another development Pi. To deploy the exact packaged binary:

```sh
AGX_BINARY="$PWD/output/target/usr/bin/agx" ./scripts/deploy-tui.sh
```

The running development Pi received the packaged executable through SSH. This update did not require reflashing or rebooting it.

## Verification — November 15, 2026

Passed:

- Rust tests for CPU counter deltas/resets, memory parsing/error cases, and layouts at 20×6, 40×12, 80×24, and 120×36.
- Clippy with warnings denied, formatting, and shell/Python syntax checks.
- ARM64 release build and complete Buildroot image build with locked, vendored dependencies.
- Image checks: partition contents, boot configuration, SSH/network setup, permissions, and the ARM64 executable read back from the generated ext4 filesystem.
- Packaged binary installed on the physical Pi; its SHA-256 matched the workstation artifact.
- Live JSON identifies the Pi 5, four CPU cores, 16 GB-class RAM, the SD card, and USB networking at `10.77.0.2`.
- Automated SSH PTY tests exercised keyboard navigation, mouse tab/row clicks, wheel selection, live CPU updates, resizing, ASCII/no-color/no-mouse modes, and noninteractive refusal.
- Terminal state was restored after `q`, Ctrl-C, and SIGTERM. Abrupt SSH disconnect terminated the TUI, including a background-job case.
- A short idle sample of the packaged binary at 120×36 measured 3,006,464 bytes of resident memory (about 2.9 MiB) and 1.44% of one CPU core. This is a brief bring-up observation, not a release benchmark.

The terminal test can be repeated against the development Pi. It creates and cleans up its own TUI sessions:

```sh
python3 -m venv .cache/qa-venv
.cache/qa-venv/bin/pip install -r tests/requirements-tui.txt
.cache/qa-venv/bin/python scripts/test-tui-ssh.py
```

Generated evidence lives in `.cache/evidence/`: `phase2-status.json`, `phase2-pty.json`, terminal captures (`phase2-overview`, `phase2-hardware`, `phase2-80x24`, `phase2-40x12`), `phase2-image-validation.txt`, and `phase2-sdcard.sha256`.

Historical TUI foundation image at this verification checkpoint: `output/images/sdcard.img`. SHA-256 (superseded by the Wi-Fi image):

```text
3327cc47e0ae78ef7953e6a661a1b7799e4104bd0133495b0dc8a0d70a9c3e11
```

The new image has passed artifact validation. A fresh cold boot from this updated image has not been performed; the packaged TUI itself was tested on the existing SD boot.

## Remaining Phase 2 work

Complete the remaining kernel driver and firmware audit, and trim the kernel while preserving USB SSH and working Broadcom Wi-Fi. Verify the required devices physically. Interactive HDMI/UART recovery remains a separate outstanding check; headless operation is the primary acceptance path. Wi-Fi management and reboot are already available. Broader service controls and the agent screen arrive with their corresponding later phases.
