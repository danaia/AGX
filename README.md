# AGX OS

**Our mission: make building enterprise-level software unbelievably easy, starting with a Raspberry Pi.**

AGX OS aims to turn a tiny, affordable computer into a powerful workspace where multiple AI agents work together to build sophisticated applications. The goal is to orchestrate lightweight, low-cost models so each task uses only the intelligence and resources it needs—keeping hardware requirements, running costs, and setup effort to a minimum.

From first boot to your first application, we want to make ambitious software development accessible to anyone, at a fraction of the cost of a traditional setup.

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
