# **AGX OS — MVP Development Plan**

Version 0.4 plan · Raspberry Pi 5 (16 GB) · Rust · Buildroot · Herdr

**Goal:** Build a headless, bootable, agent-native operating system with Wi-Fi, a colorful Rust terminal user interface (TUI), a scriptable Rust CLI, and an OpenAI-connected agent runtime. The TUI starts in Phase 2 and grows throughout the remaining phases. A graphical interface is planned after the headless MVP; this MVP does not require a desktop, GNOME, X11, Wayland, or browser.

**Current position:** Phase 1 passed through physical Pi 5 boot and SSH over USB. Phase 2 remains in progress, with Profile, Overview, Hardware, and Network screens running on the Pi. Profile is the first tab and provisions administrator accounts, persistent hostname overrides, and passwords with an advisory quality meter. SSH and console password login and the branded ASCII splash are physically verified; root SSH key recovery is retained. See the [profile guide](../docs/profile-provisioning.md). At the user's request, personal Wi-Fi provisioning and reboot were brought forward from Phases 3–4. Authentication entered through the TUI, DHCP, DNS, HTTPS, clock synchronization, Wi-Fi SSH, and automatic Wi-Fi reconnection after a real TUI-initiated reboot passed on November 15, 2026. The hardware audit is underway: SD, USB, memory, and Wi-Fi were observed on the Pi, and the first trimmed kernel removes unused RAM block devices and has booted successfully on the Pi with USB SSH and Wi-Fi restored. The remaining hardware and radio audit is outstanding; see the [hardware audit](../docs/phase2-hardware-audit.md). See the [TUI foundation record](../docs/phase2.md) and [Wi-Fi provisioning record](../docs/wifi-provisioning.md).

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

### Phase 1 — Bootloader and boot image

- Initialize Buildroot using `make raspberrypi5_defconfig`.
- Retain the existing Pi 5 EEPROM bootloader and verify it supports SD boot. Configure `config.txt`, the Pi 5 device trees, and the ARM64 kernel image.
- Produce a bootable AGX microSD image.
- Verify kernel boot messages through a console.

**Pass:** Pi boots into a working text console.

**Status:** Passed on November 15, 2026 using the user-selected SSH-over-USB path on a physical Pi 5. Kernel boot log, read/write SD root filesystem, runtime page size, USB link, and key-authenticated shell were verified. Interactive HDMI and UART recovery login remain unverified; see the [hardware observations](../docs/phase1.md#hardware-observations).

### Phase 2 — Rust TUI foundation, kernel, and hardware

**First deliverable: a usable Rust TUI on the running Pi.**

**Status:** First deliverable passed on November 15, 2026. Rust tests, lint checks, ARM64 compilation, Buildroot packaging, and SSH terminal interaction tests passed. Mouse input, resizing, keyboard controls, terminal restoration, fallback modes, and exit after SSH disconnect were exercised on the physical Pi. Personal Wi-Fi provisioning and confirmed reboot were subsequently implemented and physically verified, including saved authentication and automatic reconnection after reboot. Broadcom firmware and driver support are working. Full Phase 2 hardware acceptance remains pending.

- Preserve the Profile-first provisioning flow: account name, hostname, username, masked password/confirmation, advisory quality meter, and explicit apply review. Simple passwords are allowed. Maintain native SSH/console authentication and the branded login splash.
- Add a Rust workspace and `agx-tui`, with the initial `agx tui` launcher and a shared system-information layer. Select and pin the terminal rendering/input dependencies during implementation.
- Build the navigation shell, palette, icon fallbacks, keyboard shortcuts, and mouse event handling. Deliver Overview and Hardware screens with live uptime, CPU/memory information, storage, and USB network status.
- Show loading, empty, stale, and error states. Keep refresh work separate from input handling so keyboard and mouse remain responsive.
- Cross-compile and package the TUI in the Buildroot image, then verify it over the existing USB SSH connection before kernel trimming.
- Configure the minimum Linux ARM64 kernel.
- Enable SD storage, USB, keyboard, serial, networking, and Broadcom Wi-Fi drivers.
- Include required hardware firmware.
- Remove unused desktop and graphical packages.

**Pass:** The Pi boots headlessly, mounts its filesystem, detects the required hardware, and preserves USB SSH access. `agx tui` displays real system data on the Pi over SSH with working keyboard navigation, mouse selection/scrolling, resize handling, and clean terminal restoration. No local display is required.

### Phase 3 — Rust system initialization

A System tab, measured SSH recovery readiness, and animated TUI reboot flow have been introduced ahead of the full service manager. Workstation-built kernel/root filesystem update candidates can be packaged; safe on-device installation and rollback are still outstanding. See the [System record](../docs/system.md).

- Create `agx-init` in Rust to manage startup and system processes.
- Create a minimal filesystem and service manager.
- Add logging, process supervision, and shutdown. Migrate the existing confirmed TUI reboot operation and supervised `agx-netd` from BusyBox init into the Rust service manager without losing saved network authentication.
- Keep a recovery shell available and preserve SSH access during the init transition.
- Extend the TUI with Services and Logs screens: service health, start/stop/restart controls, and live logs with scrolling and filtering. Show operation results and service failures clearly.
- Verify that service supervision continues when the TUI exits, crashes, or loses its SSH connection.

**Pass:** AGX initializes through its Rust service manager, remains accessible headlessly, and exposes real service state, controls, and logs through the TUI. Closing the TUI does not stop system services.

### Phase 4 — Wi-Fi and networking

**Status:** The initial networking acceptance gate passed ahead of schedule on November 15, 2026: TUI provisioning, saved personal Wi-Fi authentication, DHCP/DNS/NTP, OpenAI HTTPS reachability, USB and Wi-Fi SSH, and automatic reconnection after a TUI reboot. Continue the remaining Phase 2 and Phase 3 work before extending networking.

- Extend the existing Rust `agx-net` library, `agx-netd` service, and Network screen; preserve the verified provisioning flow.
- Implement Wi-Fi scanning, connection, reconnection, and status commands.
- Use the existing Linux driver and a minimal WPA authentication service initially.
- Add DHCP, DNS, clock synchronization, and HTTPS connectivity.
- Retain root-only credential storage, masked TUI input, saved-profile controls, and secret-free logs/command arguments. Extend authentication methods only with explicit UI support and validation.
- Add a TUI Network screen for Wi-Fi scanning, selection, masked credential entry, connection progress, reconnect, and network diagnostics. Keep secrets out of logs and status messages.
- Preserve USB SSH as a recovery path while changing Wi-Fi settings; show connection loss and reconnection state in the TUI.

**Pass:** The Pi automatically connects to Wi-Fi after reboot and reaches OpenAI over HTTPS. Wi-Fi can be configured and diagnosed through the TUI, with USB SSH retained for recovery.

### Phase 5 — Complete Rust CLI and TUI administration

- Expand the initial `agx` launcher into `agx-cli` using Rust and `clap`.
- Implement `agx status`, `agx wifi`, `agx logs`, `agx reboot`, and `agx agent`.
- Support machine-readable JSON output.
- Connect the CLI to Rust system services through the shared interfaces used by the TUI.
- Complete the TUI administration workspace across Profile, Overview, Hardware, Services, Logs, and Network, including keyboard help, mouse interactions, consistent status colors/icons, and confirmations for disruptive actions.
- Keep JSON output free of terminal styling and TUI control sequences. Verify behavior in interactive and noninteractive SSH sessions.

**Pass:** AGX can be administered headlessly through either its Rust TUI or native CLI. Common tasks work with mouse or keyboard, and scripts receive stable machine-readable JSON.

### Phase 6 — Herdr agent integration

- Build `agx-agent`, a minimal Rust agent worker using Tokio and an HTTPS client.
- Integrate OpenAI API authentication and requests.
- Create a permission-controlled tool execution interface.
- Connect the Pi worker to the Herdr orchestration host.
- Allow an agent to inspect AGX system health and report results.
- Add a TUI Agent screen for requests, streaming responses, task state, tool activity, cancellation, and results.
- Present approval requests with the exact proposed action and its scope before executing permission-controlled tools. Preserve approval decisions across the CLI, TUI, and orchestration interfaces.

**Pass:** An OpenAI-powered agent responds through the CLI and TUI and executes approved diagnostic tasks. Tool approvals, progress, cancellation, and results are visible through the headless interface.

### Phase 7 — MVP release

- Produce a reproducible `agx-v0.1.0` image.
- Verify headless cold boot, USB SSH recovery, Wi-Fi recovery, TUI, CLI, and agent execution.
- Run terminal acceptance checks for keyboard-only operation, mouse clicks/scrolling, resize and narrow layouts, icon/color fallbacks, interruption, and reconnect. Test from the supported SSH client terminals.
- Verify that ordinary boot, configuration, administration, and agent use require no local monitor or graphical session.
- Record boot time, idle and active TUI resource use, agent resource use, and image size.
- Publish source, configuration, build instructions, and test results.

**Pass:** Power on Pi → boot AGX headlessly → connect over SSH → use the Rust TUI or CLI → configure networking → execute an approved agent task.

### After the headless MVP — GUI system

- Add a graphical interface as a later project after the headless MVP is accepted. Choose its framework and display stack at that time.
- Reuse the Rust service interfaces, permission model, operations, and visual language established by the TUI.
- Keep the TUI, CLI, and SSH administration available alongside the GUI. The OS must continue to boot and operate headlessly.

### Instructions for Herdr

Complete the remaining phases sequentially, preserving the user's explicit decision to bring Wi-Fi provisioning and reboot forward into Phase 2. A completed feature from a later phase does not imply the intervening phases are complete. Each phase requires source code, tests, a Git commit, and verified acceptance criteria before advancing. Do not report physical hardware tests as passed without evidence. Begin Phase 2 with the TUI vertical slice, then extend it in each later phase alongside the corresponding services. Each phase must meet the shared TUI interaction requirements for the screens it introduces. Do not introduce a graphical interface or custom kernel during the headless MVP; the GUI is a later project.

One key architectural choice: **Herdr should orchestrate development from your main workstation, while AGX runs a small Rust agent worker on the Pi.** This avoids trying to port the entire Herdr orchestration system before the operating system itself works.

The result of this MVP will be a real, bootable **AGX Linux-based operating system**, with an independent Rust control plane. Developing a completely Rust-native kernel can be a separate future project.

## Phase 1 implementation review

Buildroot 2026.08.1 is pinned with an AGX external tree derived from its Pi 5 defconfig. BusyBox init provides the initial HDMI/USB and debug UART recovery consoles; Rust init remains Phase 3. At the user's request, Phase 1 also includes key-authenticated SSH over USB Ethernet gadget mode (workstation USB-A to Pi USB-C), ahead of the full networking work. Keep the upstream board kernel until baseline boot is verified. A successful build is separate from physical boot acceptance. See [build instructions and acceptance evidence](../docs/phase1.md).
