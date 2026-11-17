---
name: angi-pi-ethernet
description: Access and control the ANGI development Raspberry Pi over direct Ethernet from this repository's workstation. Use for SSH, file transfer, VNC desktop interaction, deployment, reboot verification and connection recovery.
---

# ANGI Pi over Ethernet

Read `AGENTS.md` and `docs/pi-ethernet.md` from the repository root before
connecting. The latter is the canonical command procedure. Resolve these paths
from this skill's repository, even if the shell is in another directory.
Claude's `.claude/skills/angi-pi-ethernet` points to this same directory;
maintain one copy.

1. Establish execution context. Workstation checkout: `/home/dana/dev/AGX`.
   On-Pi checkout: `/var/lib/agx-data/angi-dev`. If already on the Pi, perform
   local operations there using `apps/agent/skills/angi-development/SKILL.md`;
   workstation NetworkManager and tunnel commands do not belong on the Pi.
2. On the workstation, check the route to `10.78.0.2`. It must use `enp1s0`
   with source `10.78.0.1`. Use the existing `agx-ethernet` profile if it needs
   activation; do not create a second profile or change the default route.
3. Load the guide's Bash connection block. Use `root@10.78.0.2`, the existing
   `~/.ssh/id_ed25519`, `HostKeyAlias=10.77.0.2`, batch authentication, bounded
   connection timeouts and strict host-key checking. Verify remote `eth0`
   and the NVMe mount before deployment. Never bypass a host-key mismatch.
4. Choose the requested operation: `pi_ssh` for commands, `pi_scp` with
   legacy SCP for transfers, the Ethernet OpenSSH tunnel for VNC, or the
   documented component deployment target with exported `AGX_SSH_*` values.
   Keep remote shell programs quoted so expansion happens on the intended host.
5. For requested changes, use the standing deployment authorization. Preserve
   the current release for rollback, install through the existing component
   script, and refresh the authenticated desktop or reboot only as needed.
   A connection-only task does not require a reboot or redeployment.
6. Verify the requested behavior and report the endpoint, checks, and any
   untested physical HDMI/input behavior. On failure, follow the guide's
   bounded diagnostics and recovery; stop and report the failing layer if
   Ethernet remains unavailable. Use USB or Wi-Fi only as an explicit recovery
   choice, retaining the same verified host identity.

Pi invariants: Buildroot/Rust init, no `apt` or `systemctl`; preserve SSH and
network access, user data, `/lib`, `/lib64`, the loader and SD boot scripts.
Keep VNC on Pi loopback, accessed at workstation `127.0.0.1:5941` through
OpenSSH. Remmina's built-in SSH tunnel stays off.
