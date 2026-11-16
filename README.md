# AGX OS

A text-only Raspberry Pi 5 operating environment built with Buildroot, with Rust system services planned for later phases.

Phase 1 is in progress. The development image supports SSH over a direct Ethernet cable at `10.77.0.2`, with HDMI/USB recovery. The repository contains a pinned Buildroot setup, Pi 5 boot configuration, and an image validation command. No hardware boot has been verified yet.

On an Arch Linux workstation with the base development tools installed:

```sh
sudo pacman -S --needed base-devel bc wget cpio unzip rsync python openssh
make configure
make dependencies
make build
make check-image
```

By default, the build authorises `~/.ssh/id_ed25519.pub` for root SSH login. Select a different public key with `SSH_PUBLIC_KEY=/path/to/key.pub make configure` and use the same value for builds. Only the public key is copied to an ignored local overlay.

The generated microSD image is `output/images/sdcard.img`. Build as your regular user; generated sources and artifacts are ignored by Git. The first build downloads a cross toolchain, firmware, Linux sources and host utilities and may take substantial time. `JOBS=4 make build` reduces build parallelism (default: 8).

See [Phase 1 build and boot instructions](docs/phase1.md) and the [MVP plan](plans/plan1.md).
