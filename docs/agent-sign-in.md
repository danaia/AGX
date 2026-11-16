# ChatGPT sign-in and NVMe data storage

The development Pi has a dedicated ext4 filesystem on `/dev/nvme0n1p1`, mounted at `/var/lib/agx-data`. Rust init mounts the volume at startup by the UUID in `/etc/agx/data-volume.uuid`. If it is missing or fails to mount, the Pi still boots with its normal recovery access. Existing SD partitions remain the boot and system volume.

At the user's request, the earlier Windows partitions and files on the 256 GB SK hynix NVMe were erased. The new filesystem is labelled `AGX_DATA`, uses an ext4 journal, and mounts with `noatime,nosuid,nodev`. E2fsprogs is included in the Buildroot configuration for administering ext4 volumes. The drive's low measured read performance should be investigated before using it for performance-sensitive workloads.

The official Codex CLI 0.169.0 ARM64 Linux binary is installed at `/var/lib/agx-data/bin/codex`, with `/usr/bin/codex` linking to it. The downloaded release archive was verified against GitHub's published SHA-256, and the extracted binary's SHA-256 was checked again on the Pi. The CLI is installed on the development Pi; a new image does not automatically download Codex or format an NVMe drive.

For an individual Pi account, run `codex login --device-auth`, open the URL shown, and approve the one-time code in a browser. The session is tied to that user's home directory; don't share or commit authentication files. A user can check `codex login status`, then run `codex` in an interactive terminal. Device-code access may need to be enabled in ChatGPT security settings. See [OpenAI's Codex authentication guide](https://developers.openai.com/codex/auth/).

This provides the official subscription-backed Codex terminal. The AGX Agent screen, `agx agent` command, restricted system-health tools, and Herdr integration remain Phase 6 work. OpenAI also documents a [Sign in with ChatGPT flow for open-source applications](https://developers.openai.com/siwc/token-sharing-open-source/) that can support eligible subscription-backed inference in an AGX-specific runtime; it is separate from reusing the Codex CLI's private credentials.
