# System tab and update candidate

The System tab (`5`) shows the running kernel and the latest measured SSH recovery readiness time. The boot metric starts when Linux starts, and ends when Dropbear is listening and `usb0` has `10.77.0.2`. EEPROM/firmware startup and host USB enumeration are outside that measurement. If the marker is missing, the tab reports that it will be available after the next boot. `S51agx-boot-metrics` records it under `/run`, rather than confusing an old measurement with the current boot.

`F10` requests reboot with a confirmation. Once the service accepts it, the TUI restores the terminal, reveals the AGX ASCII logo, and tells the user that the system is shutting down and to reconnect. A rejected request stays in the TUI and displays the error.

The chosen update source is a Buildroot build on the workstation. Run `make system-update-package` to build and validate the image and create `output/updates/agx-system-update.tar.gz`. It contains the kernel, full root filesystem image, and a manifest of component sizes and SHA-256 hashes.

**The package is a candidate, not an installable update yet.** The System tab does not claim to install it. A safe on-device installer still needs authenticated transfer and verification, preservation of accounts, SSH host keys, Wi-Fi credentials and device settings, compatibility checks, persistent rollback, interruption recovery, and hardware testing. The archive's hashes detect accidental corruption; they do not authenticate its source. Do not flash its root filesystem over the running installation: that would overwrite existing data. A full update is not complete until all of these paths are implemented and tested.

On November 15, 2026, the actual Pi rebooted through the TUI, displayed the shutdown logo and message, and reconnected over USB SSH. The boot measurement read 2.54 seconds; the TUI displayed it. Wi-Fi also reconnected. Rust tests, Clippy, ARM64 cross-compilation, and SSH TUI regression tests passed.
