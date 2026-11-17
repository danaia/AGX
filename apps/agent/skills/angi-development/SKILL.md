---
name: angi-development
description: Develop and administer ANGI on its Raspberry Pi, including native Rust and Slint builds, desktop apps, Buildroot packages, services, and privileged installation.
---

# ANGI development on the Pi

The user has authorized this workshop to extend ANGI and install applications,
including changes that need root. Run OpenCode as the desktop account. Use
`angi-root COMMAND ARG...` for privileged operations; it grants full root to
members of `agx-admin`. Quote shell programs explicitly, for example
`angi-root /bin/sh -c '...functions or redirections...'`. Ordinary builds run
without root. Apply this capability to the user's requested work.

## Find the system

- When running on the Pi, operate locally; do not run workstation connection
  setup or SSH back into this device. For workstation-side access, Claude and
  Codex share `.agents/skills/angi-pi-ethernet/SKILL.md` and
  `docs/pi-ethernet.md` in the source checkout. Direct RJ45 Ethernet uses Pi
  `10.78.0.2/24` on `eth0` and workstation `10.78.0.1/24`; `10.77.0.2` is the
  separate USB recovery address and recorded SSH host-key alias. Preserve
  Ethernet and SSH access during native installation.
- Source checkout: `/var/lib/agx-data/angi-dev`. Read its `AGENTS.md` and
  `docs/code-structure.md` before editing source.
- Public name: ANGI. Keep `agx`, `AGX_*`, crate names and system paths as
  technical identifiers.
- This is a minimal Buildroot system, with a custom Rust init; use neither
  `apt` nor `systemctl`. Inspect existing supervisor configuration and docs.
- NVMe is `/var/lib/agx-data`, mounted `nosuid,nodev`. Keep source, packages,
  compiler, build output, caches and application data there. The small root
  filesystem and boot partition are needed for recovery.
- `angi-build cli`, `angi-build gui`, `angi-build test`, and
  `angi-build gui-test` compile/test natively on the Pi. `angi-build env`
  prints the environment script path for custom native builds. Source that
  script before invoking Cargo, cc, c++, make or pkg-config directly.
- Native SDK: `/var/lib/agx-data/angi-sdk`; its private sysroot matches the
  running ANGI image. New native libraries need a matching development
  installation in this sysroot as well as the live runtime.

## Coordinate three roles

The existing peers are `orchestrator`, `builder`, and `tester`. Do not create
additional agents. All share this source checkout; orchestrator assigns file
ownership and serializes edits that overlap.

`agent tell builder 'Implement ...'` sends durable peer mail. Replace builder
with either other role. Messages are delivered as a new prompt when that
peer becomes idle. `agent inbox` reads your last 30 messages and `agent status`
shows live agents. Include paths, relevant diff/commit, commands, outcomes and
the next action. After reporting to a peer, end your turn so queued mail can
arrive. Avoid acknowledgments that cause endless reply loops.

The orchestrator handles the user conversation and scope, delegates concrete
work, collects test evidence, and reports completion. Builder implements and
provides a reviewable diff. Tester independently checks relevant behavior,
reports actionable failures, and verifies fixes. Tests may run concurrently
with read-only investigation; don't race installation or source edits.

## Build and install

Build outputs live in the account's private NVMe Cargo target directory. The
GUI is a standalone Cargo workspace; root workspace tests do not cover it.
`angi-build gui-test` uses the LinuxKMS configuration and headless unit tests.
For image/kernel changes, read `scripts/build.sh` and the board defconfig;
the checked-in image configuration currently uses a workstation cross SDK.
It needs a native-host Buildroot toolchain configuration before an image can
be rebuilt on the Pi. Do not claim an image was rebuilt from a GUI-only build.

For deployment, inspect the existing `/usr/bin/agx` and `/usr/bin/agx-gui`
links and supervisor configuration first. Stage versioned output on NVMe,
keep the prior working executable, then switch links atomically with
`angi-root`. Restart only the affected application/service. Retain SSH,
networking and the automatic HDMI/headless renderer choice while making
desktop changes. Preserve `/lib`, `/lib64`, libc and the dynamic loader;
runtime application libraries belong in a versioned private directory.
Buildroot package definitions under `buildroot/package/` make new apps
reproducible in future images; desktop catalog integration is in
`crates/agx-gui/src/launcher.rs`.

Inspect logs and exit status after changes. Run relevant existing tests and
`python3 scripts/check-file-sizes.py`. For GUI/input changes, distinguish
headless/VNC verification from physical HDMI validation; describe untested
hardware behavior accurately. Keep the previous release for rollback.
