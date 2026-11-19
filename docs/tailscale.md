# Tailscale for Agent Angi's LM Studio

Agent Angi can build with a model served by LM Studio on another computer. The two machines
reach each other privately over Tailscale; nothing is exposed to the Internet.

## Why userspace networking

The ANGI kernel has no TUN device, and rebuilding it would put SSH and recovery at risk. Tailscale
therefore runs with `--tun=userspace-networking`: it adds no network interface, routes, DNS or
firewall rules, so the Pi's Ethernet, Wi-Fi and SSH are untouched. Nothing on the device can use
the tailnet transparently; the only use is outbound, through a loopback relay.

* **Shields up.** The node is configured so nothing on the tailnet can connect in. This matters:
  in userspace mode incoming tailnet connections would otherwise reach the Pi's loopback
  services, including the desktop VNC (loopback, no password, "SSH tunnel only").
* **Relay.** `127.0.0.1:1234` (BusyBox `inetd`, loopback only) runs `tailscale nc` to the address
  chosen in the LM Studio panel. OpenCode's `lmstudio` provider points at that port.
* **No auth key.** Sign-in is the account owner's step, in a browser; ANGI never stores one.

## Install

```bash
export AGX_SSH_HOST=root@10.78.0.2 AGX_SSH_KEY="$HOME/.ssh/id_ed25519" AGX_SSH_HOST_KEY_ALIAS=10.77.0.2
./scripts/deploy-tailscale.sh
```

The pinned release is checked against its published SHA-256 first. Binaries go to a versioned
release under `/var/lib/agx-data/tailscale/releases/` and are linked into `/usr`; the boot scripts
(`apps/tailscale/`) are regular files in `/usr/libexec/agx-tailscale/` so init finds them before
the data volume mounts. The state (node key) lives in `/var/lib/agx-data/tailscale/state` (0700).
Set `AGX_TAILSCALE_OPERATOR` if the desktop user is not `dana`; that user can then sign in
without root.

`agx-init` supervises the service as `tailscale` (see `agx services`) wherever the script is
installed, so it restarts after a crash and starts at boot. Init is PID 1, so a newly installed
init takes effect at the next boot. Until then `deploy-tailscale.sh` starts the daemon directly.

## Use

1. Open **LM Studio** below the Agent Angi prompt and choose **Sign in to Tailscale**. Approve it
   in the browser window with your Tailscale account. The panel shows the device as connected.
2. On your computer, in LM Studio load a model, start the server and turn on **Serve on local
   network**.
3. Enter that computer's Tailscale name or address (for example `your-computer` or
   `100.x.y.z:1234`; the default port is 1234) and choose **Test and connect**. The test goes
   through the same relay builds use. Embedding models are not offered.
4. Choose **Use** to make LM Studio the one service Agent Angi builds with, then pick a model
   for each role.

## Checks and recovery

```bash
tailscale --socket=/run/tailscale/tailscaled.sock status          # on the Pi
agx services --json | grep -A3 tailscale                           # after the supervised boot
ip route                                                           # unchanged by Tailscale
```

* **Rollback.** Re-run the deploy with an earlier pinned version, or remove the two links
  (`/usr/bin/tailscale`, `/usr/sbin/tailscaled`) and `/usr/libexec/agx-tailscale/run`, then
  reboot. Deleting `/usr/libexec/agx-tailscale/run` alone stops init defining the service.
* **Init rollback.** `/root/agx-init-backup/agx-init.before-tailscale` is the init that preceded
  the service definition.
* **Sign out.** `tailscale --socket=/run/tailscale/tailscaled.sock logout`, and remove the device
  from the Tailscale admin console.
