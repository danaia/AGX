# **AGX OS — MVP Development Plan**

Version 0.5 plan · Raspberry Pi 5 (16 GB) · Rust · Buildroot · Herdr

**Last updated:** November 16, 2026, from the `p1` branch (31 commits since `main`, November 15–16, 2026; preview tag `v0.4.0-preview.1`), plus the library refactor (commit `b080816`) deployed to the development Pi.

**Goal:** Build a headless, bootable, agent-native operating system with Wi-Fi, a colorful Rust terminal user interface (TUI), a scriptable Rust CLI, and an OpenAI-connected agent runtime. The TUI starts in Phase 2 and grows throughout the remaining phases. The MVP does not require a desktop, GNOME, X11, Wayland, or browser. At the user's request a graphical desktop (`agx-gui`) was started ahead of Phases 6 and 7; it is additive and must never become a requirement for booting, administering, or accepting the headless MVP. See [GUI system](#gui-system--started-early-in-progress).

**Current position:** Phases 1–4 are implemented and were verified on the physical Pi over SSH; Phase 5 is mostly complete (the `agx agent` command and Agent screen wait on Phase 6); Phases 6 and 7 are not started. Rust `agx-init` is PID 1 with supervised services, Services/Logs screens, shared CLI/JSON controls and confirmed reboot/shutdown. Accounts, passwords, hostname, Wi-Fi configuration, SSH host keys and the root recovery key were checked unchanged across the init migration and a real reboot. See the [Phase 3 guide](../docs/phase3.md). Update downloading, installation and card migration are explicitly on hold. Herdr agent integration remains Phase 6 work. The Pi has a dedicated NVMe data volume and the Codex CLI installed for ChatGPT subscription sign-in; API keys are not part of the selected authentication flow.

The newest work on this branch (November 16) is the graphical desktop: a Slint/DRM-KMS tiling shell, graphical login on HDMI, V3D GPU rendering, and GUI/TUI session switching. Its GPU initialization was checked headlessly on the Pi, but **no HDMI display was connected, so on-screen operation, frame rate, and recovery after a boot-dependency incident are still unverified** (details below).

### Progress at a glance

| Phase | State | Evidence / what is left |
| --- | --- | --- |
| 1 — Bootloader and boot image | **Passed** (Nov 15) | SSH over USB on a physical Pi 5. HDMI/UART recovery login unverified. |
| 2 — Rust TUI, kernel, hardware | **Passed** (headless path, Nov 15) | TUI, trimmed kernel (RAM disks off), Wi-Fi firmware verified. Wired Ethernet, HDMI/UART login, wider radio audit open. |
| 3 — Rust system initialization | **Implemented and verified** (Nov 15) | Rust PID 1 on the Pi. Physical power-off cycle and HDMI/UART login untested. |
| 4 — Wi-Fi and networking | **Gate passed early** (Nov 15) | Rechecked under Rust init. Radio and wired-network audits remain. |
| 5 — CLI and TUI administration | **Mostly done** | `agx agent` and the Agent screen depend on Phase 6. |
| 6 — Herdr agent integration | **Not started** | Interim only: Codex CLI on the NVMe volume for subscription sign-in. |
| 7 — MVP release | **Not started** | Groundwork: `v0.4.0-preview.1` candidate, boot timing, `make check-image`. |
| GUI (outside original scope) | **In progress** (Nov 16) | Built and deployed; GPU context verified headlessly; HDMI, login, FPS and boot recovery unverified. |

### Work completed outside the original phase boundaries

- **Profile provisioning (Phase 2):** account, hostname, username, masked password with advisory meter, explicit apply review; branded animated login splash. Password login verified. See the [profile guide](../docs/profile-provisioning.md).
- **Wi-Fi provisioning and reboot (brought forward from Phase 4/3):** TUI scan/select/masked credentials, saved profiles, automatic reconnect after reboot, root-only credential storage. See the [Wi-Fi guide](../docs/wifi-provisioning.md).
- **Kernel trim and hardware audit:** RAM block devices disabled, USB gadget/SDIO retained; trimmed kernel booted on the Pi with backups kept. Fresh image not flashed. See the [hardware audit](../docs/phase2-hardware-audit.md).
- **System tab, boot timing and update candidates:** kernel and SSH-recovery boot time (2.54 s measured on Nov 15), `make system-update-package`, workstation staging and SHA-256 verification with ASCII progress, anonymous public GitHub release checks with an optional token, and `make release-update`, which strips the root recovery key and checks for secrets. The `v0.4.0-preview.1` release is public. **Installation and rollback remain unavailable**, and the archive must not be flashed over a running installation. See [System tab](../docs/system.md).
- **Persistent NVMe data volume:** dedicated ext4 `AGX_DATA` on `/dev/nvme0n1p1` mounted by Rust init at `/var/lib/agx-data` (boot continues if it is missing). The earlier Windows data on that drive was erased at the user's request. Low read speed still needs investigation. See [sign-in and storage](../docs/agent-sign-in.md).
- **Library code-structure refactor (commit `b080816`, deployed November 16, 2026):** `agx-core`, `agx-net` and `agx-tui` were split into responsibility-based modules (models/parsers/sampler; protocol/client/ssid/credentials; app state/render/screens, runtime, terminal, worker, palette). The TUI `lib.rs` went from 1,293 to 46 lines. Public APIs are preserved, and the 52 tests pass (23 in the root workspace, 29 in the standalone GUI workspace; re-run November 16, 2026). The rebuilt `agx` CLI/TUI binary (SHA-256 `7a1749c0…`) was installed on the Pi over Wi-Fi, with the previous binary kept as `/usr/bin/agx.prev`. `test-tui-ssh.py` passed (mouse, keyboard, resize, ASCII/no-color, terminal restore on q/Ctrl-C/SIGTERM, non-interactive refusal, SSH-disconnect exit) and `test-wifi-tui-ssh.py` passed (scan, masked password, dialogs, private permissions). The services and profile suites were **not** re-run: they kill the network service and create a temporary account, so they should wait for a USB recovery connection. `agx-netd`, `agx-init` and `agx-gui` were not redeployed (behavior-neutral change; restarting networking over the only link was not worth the risk). `AGENTS.md` and `CLAUDE.md` now point Codex and Claude at [docs/code-structure.md](../docs/code-structure.md): review a file near 500 lines, segment near 750 when responsibilities separate. `python3 scripts/check-file-sizes.py` lists the backlog.
- **ChatGPT sign-in groundwork:** official Codex CLI 0.169.0 installed on the NVMe volume (checksum verified) for device-code sign-in. This is not the AGX agent.

The Raspberry Pi 5 already has a bootloader in EEPROM. We'll use that rather than reinvent it, compile our own Linux kernel through Buildroot, and implement the AGX operating environment in Rust.

The important distinction: the Pi boot firmware and Linux kernel cannot realistically be entirely Rust in this MVP. **All new AGX-owned services, TUI screens, CLI commands, and agent components will be Rust.** Existing hardware drivers and firmware remain in place.

AGX BOOT ARCHITECTURE

**Raspberry Pi EEPROM Bootloader**

Existing firmware

**AGX Linux Kernel**

Buildroot · ARM64 · Pi 5 drivers

**AGX Rust Userspace**

Init · Wi-Fi management · shared service interfaces · CLI · TUI

**Herdr Agent Runtime**

OpenAI · Tool calls · Task execution

Buildroot provides a `raspberrypi5_defconfig`, which is the basis of the existing AGX image.

## Headless interaction and TUI requirements

- Make SSH the primary administration path, preserving USB SSH recovery alongside Wi-Fi SSH. A local monitor, local keyboard, or graphical session must not be required for normal operation or MVP acceptance. Track physical HDMI/UART recovery checks separately from the headless acceptance gates.
- Introduce `agx-tui` in Rust at the start of Phase 2, launched explicitly through `agx tui` in an interactive SSH terminal. Keep direct shell access available. Never start the TUI automatically for noninteractive SSH commands, scripts, or service processes.
- Make the TUI the complete provisioning interface for this Pi. Add authentication, configuration, credential management, and power controls within the TUI as their services arrive; ordinary setup must not require hand-editing files or entering secrets in shell commands. Keep the CLI available for automation.
- Build a colorful, icon-rich interface with clear navigation, colored status badges, readable panels, progress indicators, and live system information. Use a consistent palette and spacing throughout all phases. Show status through text and symbols as well as color.
- Support keyboard navigation and mouse input from the first release: click tabs and controls, select list rows, and scroll lists and logs. Every action must remain accessible from the keyboard when mouse reporting is unavailable.
- Support terminal resizing, small terminal layouts, and scrolling. Use Unicode icons with a text/ASCII fallback; optional Nerd Font glyphs must have fallbacks when the workstation font lacks them. Detect terminal color capabilities and provide reduced-color and `NO_COLOR` modes. Terminal fonts are supplied by the SSH client workstation.
- Restore terminal state and disable mouse capture on exit, errors, or interruption. Handle unavailable services, stale data, and service reconnection without freezing the interface. A lost SSH connection must leave services running, and reopening the TUI must load current service state. Confirm disruptive actions such as reboot or shutdown.
- Keep TUI rendering separate from system services. The TUI and CLI must use shared Rust interfaces and the same service operations; business logic and credentials belong in services, not screen widgets. A TUI failure must leave the OS and recovery shell usable. These interfaces should also support the later GUI.
- Add real screens as their underlying features arrive: Overview and Hardware in Phase 2, Services and Logs in Phase 3, Network brought forward during Phase 2 and extended in Phase 4, a complete administration workspace in Phase 5, and Agent in Phase 6. Label unavailable features clearly and never simulate successful operations.
- Validate on the physical Pi over SSH, including keyboard-only use, mouse clicks and scrolling, resizing, terminal cleanup, and operation without a graphical session. Keep the CLI and JSON output available for automation.

## **Phased task list for Herdr**

### What is next

In priority order. Items 1–3 are small and protect what already exists; 4–6 are the MVP critical path.

1. **Commit the work in hand** as separate commits: the plan/README update, then the library refactor with `AGENTS.md`, `CLAUDE.md`, `docs/code-structure.md` and the size-check script.
2. **Finish the refactor regression check.** The TUI and Wi-Fi suites passed on the Pi on November 16. Still to run, with USB SSH connected as a fallback: `test-services-tui-ssh.py` (`AGX_EXPECT_PID1=1`) and `test-profile-tui-ssh.py`. Redeploy `agx-netd`/`agx-gui` with the next functional change rather than for this one.
3. **Confirm the Pi boots after the libgcc_s/NVMe incident.** On November 16 the Pi was observed up on Rust PID 1 with the NVMe volume mounted (31 minutes of uptime), so it does boot again, but the reboot after the repair was not witnessed here. Do one deliberate cold reboot with USB SSH attached before more GUI deployments. With no monitor attached, the supervised `console` service (the GUI greeter) loops in backoff with "No connected display connector found"; this is expected and harmless but noisy.
4. **Phase 6 — the agent.** Build `agx-agent`, the permission-controlled tool interface with approvals, the ChatGPT sign-in flow (not the Codex CLI's private credentials), a health-inspection tool, then `agx agent` and the TUI Agent screen. This also closes Phase 5.
5. **Verify the GUI on a real display.** Connect HDMI, then check graphical login, GUI↔TUI switching (F8), the power menu, cursors, and 1080p frame rate against the 60 frames/s target. Record evidence in the desktop guide.
6. **Phase 7 — release acceptance.** Flash the fresh image and cold-boot it, run the terminal acceptance matrix, measure boot time, idle/active resource use and image size, and publish the results.
7. **Put the GUI in the Buildroot image** (binary, Mesa/input libraries, no NVMe dependency for boot-critical files) so a fresh image can reach graphical login.
8. **Wire GUI controls to the shared service interfaces** (services, network, profile, agent).
9. **Open hardware follow-ups:** HDMI/UART recovery login, wired Ethernet, radio audit (`txcap_blob` messages, region 99), NVMe read speed, a physical power-off cycle.
10. **Code-structure backlog:** `agx-tui/wifi.rs` (1,166 lines), `agx-net/service.rs` (1,128), `agx-gui/wm.rs` (1,092) and `agx-gui/main.rs` (903) are over the 750-line threshold; `profile.rs`, `supervisor.rs`, `agx-cli/main.rs` and `system.rs` are in the review band. Segment each when it is next changed.
11. **On hold by the user's decision:** on-device update download/installation, rollback, and card migration.

### Phase 1 — Bootloader and boot image

- Initialize Buildroot using `make raspberrypi5_defconfig`.
- Retain the existing Pi 5 EEPROM bootloader and verify it supports SD boot. Configure `config.txt`, the Pi 5 device trees, and the ARM64 kernel image.
- Produce a bootable AGX microSD image.
- Verify kernel boot messages through a console.

**Pass:** Pi boots into a working text console.

**Status:** Passed on November 15, 2026 using the user-selected SSH-over-USB path on a physical Pi 5. Kernel boot log, read/write SD root filesystem, runtime page size, USB link, and key-authenticated shell were verified. Interactive HDMI and UART recovery login remain unverified; see the [hardware observations](../docs/phase1.md#hardware-observations).

### Phase 2 — Rust TUI foundation, kernel, and hardware

**First deliverable: a usable Rust TUI on the running Pi.**

**Status:** First deliverable passed on November 15, 2026. Rust tests, lint checks, ARM64 compilation, Buildroot packaging, and SSH terminal interaction tests passed. Mouse input, resizing, keyboard controls, terminal restoration, fallback modes, and exit after SSH disconnect were exercised on the physical Pi. Personal Wi-Fi provisioning and confirmed reboot were subsequently implemented and physically verified, including saved authentication and automatic reconnection after reboot. Broadcom firmware and driver support are working. The headless acceptance path is verified; optional physical HDMI/UART login, wired Ethernet and the wider radio audit remain documented follow-ups and have not been claimed as tested.

- Preserve the Profile-first provisioning flow: account name, hostname, username, masked password/confirmation, advisory quality meter, and explicit apply review. Simple passwords are allowed. Maintain native SSH/console authentication and the branded login splash.
- Add a Rust workspace and `agx-tui`, with the initial `agx tui` launcher and a shared system-information layer. Select and pin the terminal rendering/input dependencies during implementation.
- Build the navigation shell, palette, icon fallbacks, keyboard shortcuts, and mouse event handling. Deliver Overview and Hardware screens with live uptime, CPU/memory information, storage, and USB network status.
- Show loading, empty, stale, and error states. Keep refresh work separate from input handling so keyboard and mouse remain responsive.
- Cross-compile and package the TUI in the Buildroot image, then verify it over the existing USB SSH connection before kernel trimming.
- Configure the minimum Linux ARM64 kernel.
- Enable SD storage, USB, keyboard, serial, networking, and Broadcom Wi-Fi drivers.
- Include required hardware firmware.
- Remove unused desktop and graphical packages. *(Amended November 16, 2026: the minimal graphical runtime for `agx-gui` — Mesa V3D/EGL/GLES/GBM, libinput, xkbcommon, fontconfig, and the `vc4-kms-v3d` overlay — was added deliberately. No display server, GNOME, X11, Wayland, or browser is installed.)*

**Pass:** The Pi boots headlessly, mounts its filesystem, detects the required hardware, and preserves USB SSH access. `agx tui` displays real system data on the Pi over SSH with working keyboard navigation, mouse selection/scrolling, resize handling, and clean terminal restoration. No local display is required.

### Phase 3 — Rust system initialization

**Status:** Implemented and physically verified on November 15, 2026. The Pi boots `agx-init` as Rust PID 1 with syslog, kernel logging, SSH, two gettys, cron and networking supervised. Crash restart and child cleanup, requested stop/start/restart, protected recovery services, live lifecycle logs, TUI keyboard/mouse confirmations and filtering, and supervision after TUI exit/reopen passed over USB SSH. Accounts and settings were checked unchanged across reboot. See the [Phase 3 record](../docs/phase3.md). Physical HDMI/UART login and a physical power-off cycle remain untested; getty processes and confirmed shutdown routing are implemented.

The System tab also provides SSH recovery timing and public GitHub release metadata checks without a token. Safe on-device update installation and rollback are deferred at the user's request.

- Create `agx-init` in Rust to manage startup and system processes.
- Create a minimal filesystem and service manager.
- Add logging, process supervision, and shutdown. Migrate the existing confirmed TUI reboot operation and supervised `agx-netd` from BusyBox init into the Rust service manager without losing saved network authentication.
- Keep a recovery shell available and preserve SSH access during the init transition.
- Extend the TUI with Services and Logs screens: service health, start/stop/restart controls, and live logs with scrolling and filtering. Show operation results and service failures clearly.
- Verify that service supervision continues when the TUI exits, crashes, or loses its SSH connection.

**Pass:** AGX initializes through its Rust service manager, remains accessible headlessly, and exposes real service state, controls, and logs through the TUI. Closing the TUI does not stop system services.

### Phase 4 — Wi-Fi and networking

**Status:** The initial networking acceptance gate passed ahead of schedule on November 15, 2026: TUI provisioning, saved personal Wi-Fi authentication, DHCP/DNS/NTP, OpenAI HTTPS reachability, USB and Wi-Fi SSH, and automatic reconnection after a TUI reboot. The same Wi-Fi flow has been rechecked under Rust init; wider radio and wired-network hardware investigations remain separate follow-ups.

- Extend the existing Rust `agx-net` library, `agx-netd` service, and Network screen; preserve the verified provisioning flow.
- Implement Wi-Fi scanning, connection, reconnection, and status commands.
- Use the existing Linux driver and a minimal WPA authentication service initially.
- Add DHCP, DNS, clock synchronization, and HTTPS connectivity.
- Retain root-only credential storage, masked TUI input, saved-profile controls, and secret-free logs/command arguments. Extend authentication methods only with explicit UI support and validation.
- Add a TUI Network screen for Wi-Fi scanning, selection, masked credential entry, connection progress, reconnect, and network diagnostics. Keep secrets out of logs and status messages.
- Preserve USB SSH as a recovery path while changing Wi-Fi settings; show connection loss and reconnection state in the TUI.

**Pass:** The Pi automatically connects to Wi-Fi after reboot and reaches OpenAI over HTTPS. Wi-Fi can be configured and diagnosed through the TUI, with USB SSH retained for recovery.

### Phase 5 — Complete Rust CLI and TUI administration

**Status:** Administration implementation advanced alongside Phase 3. `clap` now handles validated CLI commands; shared interfaces expose service controls, bounded/followed/filtered lifecycle logs, profile status, Wi-Fi scanning/connection/saved profiles/region/disconnection, and confirmed reboot/shutdown, with JSON output. Services and Logs extend the TUI workspace. The `agx agent` command and Agent screen remain Phase 6 work; Phase 5 is not declared fully complete until those are connected. See the [administration guide](../docs/phase5.md).

- Expand the initial `agx` launcher into `agx-cli` using Rust and `clap`.
- Implement `agx status`, `agx wifi`, `agx logs`, `agx reboot`, and `agx agent`.
- Support machine-readable JSON output.
- Connect the CLI to Rust system services through the shared interfaces used by the TUI.
- Complete the TUI administration workspace across Profile, Overview, Hardware, Services, Logs, and Network, including keyboard help, mouse interactions, consistent status colors/icons, and confirmations for disruptive actions.
- Keep JSON output free of terminal styling and TUI control sequences. Verify behavior in interactive and noninteractive SSH sessions.

**Pass:** AGX can be administered headlessly through either its Rust TUI or native CLI. Common tasks work with mouse or keyboard, and scripts receive stable machine-readable JSON.

### Phase 6 — Herdr agent integration

**Status:** Not started. Only groundwork exists: the official Codex CLI is installed on the development Pi for ChatGPT device-code sign-in (see [sign-in and storage](../docs/agent-sign-in.md)). `agx-agent`, the permission-controlled tool interface, Herdr connection, `agx agent`, and the TUI Agent screen are all unbuilt. The GUI's agent integration, if any, follows these.

- Build `agx-agent`, a minimal Rust agent worker using Tokio and an HTTPS client.
- Integrate ChatGPT subscription sign-in and requests using a supported OpenAI flow, without requiring API keys.
- Create a permission-controlled tool execution interface.
- Connect the Pi worker to the Herdr orchestration host.
- Allow an agent to inspect AGX system health and report results.
- Add a TUI Agent screen for requests, streaming responses, task state, tool activity, cancellation, and results.
- Present approval requests with the exact proposed action and its scope before executing permission-controlled tools. Preserve approval decisions across the CLI, TUI, and orchestration interfaces.

**Pass:** An OpenAI-powered agent responds through the CLI and TUI and executes approved diagnostic tasks. Tool approvals, progress, cancellation, and results are visible through the headless interface.

### Phase 7 — MVP release

**Status:** Not started; depends on Phase 6. Existing groundwork: a validated image build (`make check-image`), a public `v0.4.0-preview.1` update candidate (not an installable release), the SSH-recovery boot-time metric (2.54 s on Nov 15; not a release benchmark), and short RSS/CPU samples for init and the TUI in the [Phase 3 record](../docs/phase3.md). The complete fresh image has been built and validated but **not flashed**; a cold boot from it, the terminal acceptance matrix, and image-size/resource records are still to do. If the GUI ships in the release image it also needs its own boot, recovery and performance acceptance (below).

- Produce a reproducible `agx-v0.1.0` image.
- Verify headless cold boot, USB SSH recovery, Wi-Fi recovery, TUI, CLI, and agent execution.
- Run terminal acceptance checks for keyboard-only operation, mouse clicks/scrolling, resize and narrow layouts, icon/color fallbacks, interruption, and reconnect. Test from the supported SSH client terminals.
- Verify that ordinary boot, configuration, administration, and agent use require no local monitor or graphical session.
- Record boot time, idle and active TUI resource use, agent resource use, and image size.
- Publish source, configuration, build instructions, and test results.

**Pass:** Power on Pi → boot AGX headlessly → connect over SSH → use the Rust TUI or CLI → configure networking → execute an approved agent task.

### GUI system — started early (in progress)

The graphical desktop was originally scheduled after the headless MVP. At the user's request it was started on November 16, 2026, alongside Phase 5/6 work. The headless MVP gates above are unchanged and are still judged without any display. Full detail is in the [desktop guide](../docs/desktop.md).

**Built (commits `89c2444`, `8de9724`, `893ef53`, `c024fef`, `7887013`, `2ccefab`):**

- `agx-gui` crate (Slint, `linuxkms` backend drawing directly to DRM/KMS — no X11/Wayland): Omarchy/Hyprland-style dwindle tiling window manager (`wm.rs`, `keymap.rs`, unit-tested), five workspaces, float/tile/fullscreen/maximize, keyboard chords and mouse move/resize, a menu, system-data panels (Overview, CPU, Memory and others), and a bundled JetBrains Mono Nerd Font.
- Graphical login: Rust init starts `agx-gui --login` on HDMI `tty1`. It authenticates the existing AGX administrator accounts, rejecting root, empty, locked and expired credentials with a two-second retry delay, then runs the desktop as that user. Images without the GUI keep the HDMI text login; SSH and serial logins are unchanged.
- Session management: `agx desktop`, power dropdown (Log out, Enter TUI, Reboot, Shut down, with confirmations), and the **F8** shortcut / **Back to GUI** action from the HDMI TUI. SSH and serial TUI sessions show it disabled.
- GPU rendering: FemtoVG on the Pi's V3D through Mesa/EGL/GBM, with explicit renderer selection (no silent software fallback) and `/etc/agx/gui.env` options for output and mode. Pointer hover and drag avoid layout rebuilds via persistent scene models. Custom directional cursors with a `--cursor-preview` mode.
- Deployment and safety tooling: `make deploy-gui` (SSH, NVMe-backed runtime so the 256 MiB SD root is not filled), `package-gui-runtime.py`, `install-gui-login-sd.sh`, `repair-gui-boot.sh`, `check-gpu.c`, `test-gui-runtime.py`; Buildroot options for the graphical runtime and display overlay.

**Verified on the Pi:** a headless GPU check (as root and as `dana`, November 16, 2026) created an EGL 1.5 / OpenGL ES 3.1 context on Broadcom V3D 7.1.10.2 with Mesa 26.1.8. Window-manager and key-map logic have unit tests. This verifies driver and context initialization only.

**Not verified — do not report as passed:**

- Any HDMI output: both ports were disconnected during testing. Graphical login, the desktop, F8/GUI↔TUI switching, the power menu, cursors, and 1080p frame rate/latency (target 60 frames/s, record dips) have no hardware evidence.
- **Boot recovery.** The first GPU deployment linked `/lib/libgcc_s.so.1` to the NVMe volume, creating a boot dependency cycle, and the Pi did not return after reboot. The packager now excludes `/lib` and `/lib64`, and deployment checks PID 1's dependencies for NVMe. The recovery script exists, but the doc still records that boot recovery after the repair needs confirmation.
- 4K output, and the SD-card installer on a physical card.

**Remaining work:**

1. Reconnect HDMI, confirm the Pi boots and returns after the repair, then verify graphical login, session switching, and frame rate; record evidence in the desktop guide.
2. Integrate the GUI into the reproducible Buildroot image. Today `agx-gui` has its own `Cargo.lock`, sits outside the workspace, and is built and deployed by separate scripts. A fresh image needs the binary and runtime libraries, and the GPU libraries currently depend on the NVMe volume.
3. Wire GUI controls to the same shared service interfaces used by the TUI and CLI (services, Wi-Fi/network, profile, and later the agent); the panels so far show system data.
4. Remaining window-manager gaps: grouping/tabs, scratchpad, multiple monitors.
5. Keep the TUI, CLI and SSH paths working and tested alongside the GUI, and keep a GUI startup failure recoverable over SSH/serial.

### Instructions for Herdr

Complete the remaining phases sequentially, preserving the user's explicit decision to bring Wi-Fi provisioning and reboot forward into Phase 2. A completed feature from a later phase does not imply the intervening phases are complete. Each phase requires source code, tests, a Git commit, and verified acceptance criteria before advancing. Do not report physical hardware tests as passed without evidence. Begin Phase 2 with the TUI vertical slice, then extend it in each later phase alongside the corresponding services. Each phase must meet the shared TUI interaction requirements for the screens it introduces. The graphical interface was started early at the user's explicit direction (November 16, 2026); it must not delay, replace, or weaken the headless acceptance gates, and its hardware claims need the same physical evidence. Do not introduce a custom kernel source tree during the headless MVP (the kernel fragments in use only trim configuration). Before any deployment that touches boot-critical paths, confirm nothing that PID 1 needs at boot is placed on the NVMe volume.

One key architectural choice: **Herdr should orchestrate development from your main workstation, while AGX runs a small Rust agent worker on the Pi.** This avoids trying to port the entire Herdr orchestration system before the operating system itself works.

The result of this MVP will be a real, bootable **AGX Linux-based operating system**, with an independent Rust control plane. Developing a completely Rust-native kernel can be a separate future project.

## Phase 1 implementation review

Buildroot 2026.08.1 is pinned with an AGX external tree derived from its Pi 5 defconfig. BusyBox init provides the initial HDMI/USB and debug UART recovery consoles; Rust init remains Phase 3. At the user's request, Phase 1 also includes key-authenticated SSH over USB Ethernet gadget mode (workstation USB-A to Pi USB-C), ahead of the full networking work. Keep the upstream board kernel until baseline boot is verified. A successful build is separate from physical boot acceptance. See [build instructions and acceptance evidence](../docs/phase1.md).
