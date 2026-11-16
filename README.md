# AGX OS

**Our mission: make building enterprise-level software unbelievably easy, starting with a Raspberry Pi.**

AGX OS aims to turn a tiny, affordable computer into a powerful workspace where multiple AI agents work together to build sophisticated applications. The goal is to orchestrate lightweight, low-cost models so each task uses only the intelligence and resources it needs—keeping hardware requirements, running costs, and setup effort to a minimum.

From first boot to your first application, we want to make ambitious software development accessible to anyone, at a fraction of the cost of a traditional setup.

## What is AGX?

**AGX is a thin operating system made for one job: letting AI coding agents build software — and build the operating system itself.**

Most operating systems are made for people and later adapted for agents. AGX starts from the other end. It is a small, fully inspectable machine whose every command, service, and log is available to an agent as plain text and JSON, running on a Raspberry Pi 5 that costs a fraction of a developer workstation. The whole system, from boot to the dashboard, lives in one repository. An agent can read it, change it, rebuild it, flash it, and verify the result. Nothing is inherited from someone else's distribution, so nothing is off-limits.

### How AGX is different

- **It is not a Linux distribution.** AGX is not Debian, Ubuntu, Arch, Fedora, or Alpine underneath, and it has no package manager or distribution release cycle. It is assembled from source with [Buildroot](https://buildroot.org/), which is a build tool rather than a distribution, so the image contains only what AGX asks for. There is no distribution to be compatible with, and nothing to uninstall before you can change something.
- **The AGX layer is written in Rust.** Startup (`agx-init`, PID 1), the service supervisor, the network and account services, the `agx` command line, the terminal dashboard, and the desktop are all Rust, about 11,000 lines in one workspace. The init program is a 0.5 MB binary.
- **Everything is scriptable.** Status, services, logs, Wi-Fi, accounts, and power each have an `agx` command with `--json` output. The terminal dashboard and the CLI use the same service interfaces, so what an agent can do is what a person can do. The desktop is meant to join them; that wiring is not finished.
- **Agents already build it.** The setup below is a single prompt: your coding agent downloads the code, builds AGX, flashes the card, and checks that it boots.
- **It is small on purpose.** A fresh image has about 93 MB of files, boots to a reachable SSH login in about 2.5 seconds after Linux starts, and used roughly 120 MB of the Pi's 16 GB of RAM while idle. These are development measurements, not release benchmarks.

### What AGX is built on

Honesty matters here, because "not a distribution" does not mean "from nothing." AGX runs the **Linux kernel** (Raspberry Pi's 6.12 branch, trimmed for the Pi 5), uses the Pi's own firmware bootloader, and relies on a few small, well-known components: BusyBox for the shell and core utilities, Dropbear for SSH, `wpa_supplicant` for Wi-Fi security, and Mesa for the desktop's GPU drivers. AGX does not rewrite these. It owns everything above them, and that is where agents do their work. A Linux kernel written in Rust is a separate, far larger project and is not part of the current plan.

### Working today and planned

| Working on a real Pi 5 | Planned |
| --- | --- |
| Bootable image built from source; SSH over USB and Wi-Fi | An AGX agent that runs on the Pi, signs in with a ChatGPT subscription, and uses permission-controlled tools (Phase 6) |
| Rust init, service supervision, logs, confirmed reboot and shutdown | Herdr orchestration of several agents across machines |
| Terminal dashboard and `agx` CLI with JSON output | A reproducible `v0.1.0` release image, with measured boot time and resource use (Phase 7) |
| Wi-Fi, accounts, and password login provisioned from the dashboard | On-device system updates with rollback (on hold) |
| Desktop with tiling windows and graphical login (built; not yet verified on a monitor) | The desktop built into the standard image |

The AGX agent does not exist yet. Today the agent is the coding agent on your computer that builds and checks the system. The [project roadmap](plans/plan1.md) records exactly what has been tested, and what has not. AGX is early software: this describes the direction, and the roadmap describes the evidence.

## Get started with one prompt

**Connect the Pi. Insert a microSD card into your computer. Give your coding agent the prompt below.**

Your agent downloads the code, installs the build tools, builds AGX OS, writes the card, and sets up the USB connection. You move the finished card into the Pi, and the agent checks that AGX is running.

AGX is in early development. The instructions below use the tested **Arch Linux + NetworkManager + Raspberry Pi 5** setup. The prompt is for a coding agent with terminal access on your computer, such as Codex. It guides the agent through the repository's build tools; there is no separate one-click installer yet.

### 1. Plug everything in

You need:

- A **Raspberry Pi 5**.
- A **USB-A to USB-C data cable**. A charge-only cable will not work.
- A **microSD card you can erase**. A 32 GB card has been tested.
- A **card reader** connected to your computer.
- An **Arch Linux computer using NetworkManager**, with internet access and a coding agent that can run terminal commands.

Connect the Pi to your computer, and put the microSD card in the computer's card reader:

```text
YOUR COMPUTER
    │
    ├── USB-A ── data cable ── USB-C on the Raspberry Pi 5
    │
    └── Card reader ── microSD card to install AGX OS onto
```

Use one AGX Pi at a time. Leave its card slot empty for now. You do not need a monitor, keyboard, or Wi-Fi connection for the Pi.

**Everything on the selected microSD card will be erased.** Disconnect other removable drives to make the card easier to identify.

### 2. Give your agent this prompt

Open your coding agent on your computer. Copy and paste this entire prompt:

```text
Set up AGX OS on my Raspberry Pi 5 for me.

The Pi is connected to this computer with a USB-A to USB-C data cable.
The microSD card to erase and flash is in this computer's card reader.
Repository: https://github.com/danaia/AGX.git

Carry out the setup, including downloads, dependency installation, building,
USB networking, flashing, and verification. Keep explanations short and simple.
Do the terminal work yourself. Ask me only for information you cannot determine,
credentials or permissions you need, or physical actions I must perform.

1. Inspect this computer's OS, available disk space, memory, network manager,
   and attached storage. Use the tested Arch Linux + NetworkManager workflow.
   If this computer needs a different workflow, explain what is needed before
   making system changes. Install the necessary host tools, including base-devel,
   git, curl, bc, wget, cpio, unzip, rsync, python, openssh, and xz.

2. Clone the repository into a suitable user-owned directory, or use an existing
   checkout without overwriting local changes. Read its README, build scripts,
   and docs/phase1.md for the current build and boot instructions.

3. Reuse an appropriate SSH key on this computer. If none exists, create an
   Ed25519 key without overwriting any existing key. Keep the private key here.
   Set SSH_PUBLIC_KEY to the matching public key for every build command.

4. Run make configure, make dependencies, make build, and make check-image as
   my normal user. Choose JOBS to suit this computer. Download the required
   toolchains through the build scripts. Resolve errors and resume the build.
   Continue only when output/images/sdcard.img passes validation.

5. Create or reuse the agx-direct NetworkManager profile for the USB adapter
   with MAC 02:77:00:00:00:01. Use 10.77.0.1/24 on this computer, no gateway,
   ipv4.never-default yes, IPv6 disabled, and autoconnect enabled. Preserve my
   normal internet connection. The Pi will use 10.77.0.2.

6. Identify the microSD card by its device path, model, capacity, transport,
   and mount points. Show me which card you identified. I authorize erasing
   and flashing that card. If its identity is ambiguous, ask me which device
   to use. Never write to a system disk or another unrelated drive.
   Recheck the device identity immediately before writing. Unmount its
   partitions, write sdcard.img to the whole card, flush writes, and verify
   the written bytes against the image. Safely eject the card when complete.

7. Tell me when the card is ready. Ask me to disconnect power from the Pi,
   move the card from the reader into the Pi, and reconnect the USB cable.
   Wait for me to say the card is in before continuing.

8. Wait for the Pi to boot, activate agx-direct if needed, and connect as
   root@10.77.0.2 using the matching SSH private key. Handle the first host-key
   trust prompt for this directly connected Pi. If a saved host key conflicts,
   verify the device before replacing that entry. Check the board identity
   and run agx status --json. Diagnose connection problems if needed.

9. Open agx tui in an interactive SSH terminal if your tools support it.
   Otherwise give me one exact command, with the correct key path, to open
   it myself. Explain the 1-7, ?, F9, F10, and q keys and how to shut down safely.
   Report what you actually verified and anything still unresolved.
```

Your agent may need your computer password or approval to install packages and write the card. Enter passwords through your computer's secure prompt. The first build downloads the tools and operating system sources, so allow time and keep your computer online.

### 3. Move the card when your agent asks

Once the agent says the card is written and verified:

1. Unplug the Pi's USB cable so the Pi is powered off.
2. Take the microSD card out of your computer's reader.
3. Put it into the Pi's microSD slot.
4. Reconnect the USB cable.
5. Tell your agent: **“The card is in and the Pi is connected. Continue.”**

```text
Before:  Computer ── card reader ── microSD
                                      │
                                move the card
                                      ↓
After:   Computer ── USB cable ── Pi [microSD]
```

The agent will check the connection and help open the AGX dashboard.

### 4. Use AGX

In the dashboard:

- Press **1** for Profile, **2** for Overview, or **3** for Hardware.
- Press **4** for Network, **5** for System, **6** for Services, or **7** for Logs.
- Press **F9** to confirm power off or **F10** to confirm reboot.
- Press **?** for help.
- Click tabs and rows, or use the arrow keys and mouse wheel.
- Press **q** to return to the Pi's terminal.

When you are finished, ask your agent to shut down the Pi, or run `poweroff` in the Pi's terminal. Wait a few seconds after the connection closes before unplugging the cable.

## If you get stuck

Tell your agent what happened and ask it to continue diagnosing the setup.

- **The Pi keeps restarting or disconnecting:** the computer's USB port may provide too little power. A suitable powered data connection may be needed. See [Raspberry Pi's USB networking and power guidance](https://www.raspberrypi.com/news/usb-gadget-mode-in-raspberry-pi-os-ssh-over-usb/).
- **The agent cannot identify the card:** give it the card's capacity and reader details. Let it inspect devices again after you remove and reinsert the card.
- **The build stopped:** ask the agent to fix the error and resume. It can reuse downloaded files and completed build work.
- **The dashboard looks wrong:** ask the agent to try `agx tui --ascii --no-color --no-mouse`.

## Project status and technical details

USB login, Rust-managed startup, the dashboard, service supervision, and Wi-Fi have been tested on a real Pi 5. A graphical desktop (`agx-gui`, with graphical login on HDMI) is under development: the Pi's GPU initializes, but on-screen operation has not yet been verified on a monitor. Multi-agent orchestration (the Phase 6 agent) is not built yet. The image uses a small, fixed filesystem, so most of a 32 GB card remains unused. HDMI and serial recovery login still need physical verification.

- [Manual build, USB connection, flashing, and recovery instructions](docs/phase1.md)
- [Dashboard controls, development, and verification](docs/phase2.md)
- [Rust startup, service controls, logs, and recovery](docs/phase3.md)
- [CLI and TUI administration](docs/phase5.md)
- [System tab, update candidates, and releases](docs/system.md)
- [NVMe data volume and ChatGPT sign-in](docs/agent-sign-in.md)
- [Desktop (GUI) status and usage](docs/desktop.md)
- [Project roadmap](plans/plan1.md)
