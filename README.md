# AGX OS

A headless Raspberry Pi 5 operating environment built with Buildroot. The first Rust TUI provides live system and hardware screens with colors, icons, keyboard navigation, and mouse support. Rust system services and further administration screens follow in later phases. A GUI is planned after the headless MVP.

Phase 1 image build, artifact validation, and a physical Pi 5 boot with SSH over USB have passed. The development image supports SSH over USB (workstation USB-A to Pi USB-C) at `10.77.0.2`, with HDMI/USB recovery. The repository contains a pinned Buildroot setup, Pi 5 boot configuration, and an image validation command. Interactive HDMI and serial recovery login remain unverified.

On the Pi's SSH shell, launch the installed console:

```sh
agx tui
```

Click Overview or Hardware, select device rows, and scroll with the mouse wheel. Keyboard: `1`/`2`/`?` select screens, Tab switches, arrows scroll, `r` refreshes, and `q` returns to the shell. For a full-color client terminal over SSH, use `agx tui --color truecolor`; `agx tui --ascii --no-color --no-mouse` provides a basic terminal fallback. `agx status --json` is available for scripts.

Phase 2's TUI foundation is installed and tested on the Pi. The hardware/firmware audit and kernel trimming remain outstanding. See [TUI development and verification](docs/phase2.md).

On an Arch Linux workstation with the base development tools installed:

```sh
sudo pacman -S --needed base-devel bc wget cpio unzip rsync python openssh
make configure
make dependencies
make build
make check-image
```

By default, the build authorises `~/.ssh/id_ed25519.pub` for root SSH login. Select a different public key with `SSH_PUBLIC_KEY=/path/to/key.pub make configure` and use the same value for builds. Only the public key is copied to an ignored local overlay.

The generated microSD image is `output/images/sdcard.img`. Build as your regular user; generated sources and artifacts are ignored by Git. The first build downloads a cross toolchain, firmware, Linux sources, a pinned host Rust toolchain, and host utilities and may take substantial time. `JOBS=4 make build` reduces build parallelism (default: 8).

See [Phase 1 build and boot instructions](docs/phase1.md) and the [MVP plan](plans/plan1.md).
