# **AGX OS — MVP Development Plan**

Version 0.1 · Raspberry Pi 5 (16 GB) · Rust · Buildroot · Herdr

**Goal:** Build a minimal, bootable, agent-native operating system with Wi-Fi, a Rust CLI, and an OpenAI-connected agent runtime. No desktop, GNOME, X11, Wayland, or browser.

The Raspberry Pi 5 already has a bootloader in EEPROM. We'll use that rather than reinvent it, compile our own Linux kernel through Buildroot, and implement the AGX operating environment in Rust.

The important distinction: the Pi boot firmware and Linux kernel cannot realistically be entirely Rust in this MVP. **All new AGX-owned services, CLI commands, and agent components will be Rust.** Existing hardware drivers and firmware remain in place.

AGX BOOT ARCHITECTURE

**Raspberry Pi EEPROM Bootloader**

Existing firmware

**AGX Linux Kernel**

Buildroot · ARM64 · Pi 5 drivers

**AGX Rust Userspace**

Init · Wi-Fi management · CLI

**Herdr Agent Runtime**

OpenAI · Tool calls · Task execution

Buildroot currently provides a `raspberrypi5_defconfig`, so there is an established board configuration to start from.




## **Phased task list for Herdr**

### Phase 1 — Bootloader and boot image

- Initialize Buildroot using `make raspberrypi5_defconfig`.
- Retain the existing Pi 5 EEPROM bootloader and verify it supports SD boot. Configure `config.txt`, the Pi 5 device trees, and the ARM64 kernel image.
- Produce a bootable AGX microSD image.
- Verify kernel boot messages through a console.

**Pass:** Pi boots into a working text console.

**Status:** Passed on November 15, 2026 using the user-selected SSH-over-USB path on a physical Pi 5. Kernel boot log, read/write SD root filesystem, runtime page size, USB link, and key-authenticated shell were verified. Interactive HDMI and UART recovery login remain unverified; see the [hardware observations](../docs/phase1.md#hardware-observations).

### Phase 2 — Kernel and hardware

- Configure the minimum Linux ARM64 kernel.
- Enable SD storage, USB, keyboard, serial, networking, and Broadcom Wi-Fi drivers.
- Include required hardware firmware.
- Remove unused desktop and graphical packages.

**Pass:** Kernel boots, mounts the filesystem, and detects the required hardware.

### Phase 3 — Rust system initialization

- Create `agx-init` in Rust to manage startup and system processes.
- Create a minimal filesystem and service manager.
- Add logging, process supervision, shutdown, and reboot.
- Keep a recovery console available.

**Pass:** AGX initializes through its Rust service manager.

### Phase 4 — Wi-Fi and networking

- Create `agx-net` in Rust.
- Implement Wi-Fi scanning, connection, reconnection, and status commands.
- Use the existing Linux driver and a minimal WPA authentication service initially.
- Add DHCP, DNS, clock synchronization, and HTTPS connectivity.
- Securely store network credentials.

**Pass:** Pi automatically connects to Wi-Fi after reboot and reaches OpenAI over HTTPS.

### Phase 5 — Native Rust CLI

- Create `agx-cli` using Rust and `clap`.
- Implement `agx status`, `agx wifi`, `agx logs`, `agx reboot`, and `agx agent`.
- Support machine-readable JSON output.
- Connect the CLI to Rust system services.

**Pass:** AGX can be administered entirely through its native CLI.

### Phase 6 — Herdr agent integration

- Build `agx-agent`, a minimal Rust agent worker using Tokio and an HTTPS client.
- Integrate OpenAI API authentication and requests.
- Create a permission-controlled tool execution interface.
- Connect the Pi worker to the Herdr orchestration host.
- Allow an agent to inspect AGX system health and report results.

**Pass:** An OpenAI-powered agent responds through the AGX CLI and can execute approved diagnostic tasks.

### Phase 7 — MVP release

- Produce a reproducible `agx-v0.1.0` image.
- Verify cold boot, Wi-Fi recovery, CLI, and agent execution.
- Record boot time, memory consumption, and image size.
- Publish source, configuration, build instructions, and test results.

**Pass:** Power on Pi → boot AGX → connect Wi-Fi → run Rust CLI → execute agent task.

### Instructions for Herdr

Complete phases sequentially. Each phase requires source code, tests, a Git commit, and verified acceptance criteria before advancing. Do not report physical hardware tests as passed without evidence. Do not introduce a graphical interface or custom kernel during this MVP.

One key architectural choice: **Herdr should orchestrate development from your main workstation, while AGX runs a small Rust agent worker on the Pi.** This avoids trying to port the entire Herdr orchestration system before the operating system itself works.

The result of this MVP will be a real, bootable **AGX Linux-based operating system**, with an independent Rust control plane. Developing a completely Rust-native kernel can be a separate future project.

## Phase 1 implementation review

Buildroot 2026.08.1 is pinned with an AGX external tree derived from its Pi 5 defconfig. BusyBox init provides the initial HDMI/USB and debug UART recovery consoles; Rust init remains Phase 3. At the user's request, Phase 1 also includes key-authenticated SSH over USB Ethernet gadget mode (workstation USB-A to Pi USB-C), ahead of the full networking work. Keep the upstream board kernel until baseline boot is verified. A successful build is separate from physical boot acceptance. See [build instructions and acceptance evidence](../docs/phase1.md).
