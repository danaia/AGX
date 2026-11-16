# System tab and update candidate

The System tab (`5`) shows the running kernel and the latest measured SSH recovery readiness time. The boot metric starts when Linux starts, and ends when Dropbear is listening and `usb0` has `10.77.0.2`. EEPROM/firmware startup and host USB enumeration are outside that measurement. If the marker is missing, the tab reports that it will be available after the next boot. `S51agx-boot-metrics` records it under `/run`, rather than confusing an old measurement with the current boot.

`F10` requests reboot with a confirmation. Once the service accepts it, the TUI restores the terminal, reveals the AGX ASCII logo, and tells the user that the system is shutting down and to reconnect. A rejected request stays in the TUI and displays the error.

The chosen update source is a Buildroot build on the workstation. Run `make system-update-package` to build and validate the image and create `output/updates/agx-system-update.tar.gz`. It contains the kernel, full root filesystem image, and a manifest of component sizes and SHA-256 hashes.

Transfer a candidate over the root SSH recovery connection with `make stage-system-update`. This verifies the transferred archive's SHA-256 and stages it in a root-owned directory readable by AGX administrators. In System, press `v` to verify the staged package. The local check inspects that package; it does not determine whether the build is newer. The ASCII bar reflects bytes hashed as the kernel and root filesystem are checked against the manifest. A root-owned, non-writable-by-others package and directory are required.

**The package is a candidate, not an installable update yet.** The System tab shows installation as unavailable and does not simulate install progress. A safe on-device installer still needs authenticated transfer and verification, preservation of accounts, SSH host keys, Wi-Fi credentials and device settings, compatibility checks, persistent rollback, interruption recovery, and hardware testing. The archive's hashes detect accidental corruption; they do not authenticate its source. Do not flash its root filesystem over the running installation: that would overwrite existing data. A full update is not complete until all of these paths are implemented and tested.

On November 15, 2026, the actual Pi rebooted through the TUI, displayed the shutdown logo and message, and reconnected over USB SSH. The boot measurement read 2.54 seconds; the TUI displayed it. Wi-Fi also reconnected. Rust tests, Clippy, ARM64 cross-compilation, and SSH TUI regression tests passed.

The check CTA was tested on the Pi with the staged candidate, including byte progress to 100% and rejection of unsafe package permissions. The candidate hashes provide integrity checking, while authenticated transfer relies on the existing root SSH key; the package is not cryptographically signed.

## GitHub release checks

The AGX repository and preview release are public. Press `c` or click **Check GitHub** without configuring a token.

An optional token can raise GitHub's API rate limit. Press `g` or click **Optional token** to save one. The TUI masks it and passes it to the root-owned service; `/etc/agx/github-token` is mode `0600`, and the token is supplied to curl through a private pipe, not command-line arguments. An empty token submitted in the dialog removes it and restores anonymous checks.

Press `c` or click **Check GitHub**. AGX contacts GitHub over HTTPS, selects the highest semantic version among up to 100 published releases including previews, and reports whether the AGX update package is attached. Release metadata is not a system installation, and the device does not yet track an installed system release version, so the result does not claim the installed system is current or outdated. `v` separately verifies the workstation-staged package. Install and install progress remain unavailable until data preservation and rollback are implemented.

The release build command `make release-update` builds and validates the candidate, removes the workstation-specific root SSH recovery key from its root filesystem, checks that no GitHub token or saved Wi-Fi profile is present, and produces a versioned candidate archive and SHA-256 file. A release archive remains a development candidate and must not be flashed over the existing installation.

The workstation’s GitHub CLI credential is not copied to the device. The release candidate is attached to the public GitHub preview release.
