# ANGI on Raspberry Pi Zero 2 W

The Zero 2 W image includes USB and Wi-Fi SSH, the ANGI TUI, a headless desktop
over VNC, and Agent Angi Chat and Project modes. Projects use one OpenCode coding
agent with OpenAI or an LM Studio server on the local network. Accounts, Wi-Fi,
SSH host keys, projects, and the private agent store persist on the SD card.
The image uses an 8 GiB root partition and requires a card of 16 GB or larger.

## Public download

Run `make release-zero2w` to build the public image and its SHA-256 checksum
in `output/zero2w/release/`. Use Zstandard to decompress the download before
flashing.

```sh
sha256sum -c angi-zero-2w.img.zst.sha256
zstd -d angi-zero-2w.img.zst
```

After flashing, open the SD card's FAT boot partition and copy your OpenSSH
Ed25519 public key to the partition as `angi-ssh.pub`. If you need a key, run
`ssh-keygen -t ed25519` and copy the resulting `.pub` file. Keep the private
key on your computer. The Pi imports the public key on first boot. The boot
partition includes these instructions in `ANGI-SETUP.txt`. Continue with
[USB provisioning](#provision-over-usb).

## Build and flash

For a development image, build from the repository root using the existing
workstation SSH key:

```sh
export SSH_PUBLIC_KEY="$HOME/.ssh/id_ed25519.pub"
make configure BOARD=zero2w
make dependencies BOARD=zero2w
make build BOARD=zero2w
make check-image BOARD=zero2w
(cd output/zero2w/images && sha256sum -c sdcard.img.sha256)
```

The image is `output/zero2w/images/sdcard.img`. This configuration is for the
Raspberry Pi **Zero 2 W**. The Pi 5 uses the default build target and a separate
image.

Use `lsblk -o NAME,PATH,MODEL,SIZE,TYPE,TRAN,RM,MOUNTPOINTS` to identify your SD
card. Confirm its identity and that it may be erased, then unmount any of its
partitions. Replace `/dev/disk/by-id/USB_CARD_READER` with the verified whole-card
device. Check the device again immediately before writing.

```sh
image=output/zero2w/images/sdcard.img
card=/dev/disk/by-id/USB_CARD_READER
sudo dd if="$image" of="$card" bs=4M conv=fsync status=progress
sudo cmp -n "$(stat -c '%s' "$image")" "$image" "$card"
```

Eject the card after the comparison succeeds. Insert it into the powered-off
Zero 2 W. The workstation's USB-C port connects to the Pi's **micro-USB data
port** using a data-capable cable. Use a suitable supply on the separate power
port if the workstation cannot power the board reliably.

## Provision over USB

On a Linux workstation with NetworkManager, create the following profile once,
or activate the matching profile if it already exists. It matches the gadget's
workstation-side MAC and leaves the normal internet route on its existing link.

```sh
nmcli connection add type ethernet con-name angi-zero-usb \
  ethernet.mac-address 02:79:00:00:00:01 \
  ipv4.method manual ipv4.addresses 10.79.0.1/24 ipv4.never-default yes \
  ipv6.method disabled
nmcli connection up angi-zero-usb
ip route get 10.79.0.2
```

Use a unique known-host alias for each physical Zero. Establish trust with the
board physically connected over the direct USB link and record its SSH
fingerprint. If the board already has a trusted key and SSH reports a mismatch,
verify it through the serial console before changing the known-host entry. The
Pi 5's existing `10.77.0.2` alias identifies a different device.

```sh
zero_id=angi-zero-setup
ssh -i "$HOME/.ssh/id_ed25519" -o IdentitiesOnly=yes \
  -o HostKeyAlias="$zero_id" -t root@10.79.0.2 'agx tui'
```

In **Profile**, create your administrator account and set the hostname,
username, and password. In **Network**, set the Wi-Fi country, scan, and use
**Connect & save** with a 2.4 GHz network. Read the DHCP address in **Network**.
To verify the named account from Wi-Fi, connect with
`ssh -o HostKeyAlias="$zero_id" -o StrictHostKeyChecking=yes USER@WIFI_IP` and
enter the account password. USB SSH remains available during Wi-Fi changes.

## Open the desktop

Use either local OpenSSH forward below from the workstation. Set `zero_id` to
the alias you trusted during USB setup and replace `WIFI_IP` with the current
address from the TUI. Only the workstation's loopback address listens for the
viewer. The Zero's VNC server likewise listens on loopback.

```sh
# USB
ssh -NT -o ExitOnForwardFailure=yes -o HostKeyAlias="$zero_id" \
  -o StrictHostKeyChecking=yes -L 127.0.0.1:5943:127.0.0.1:5900 root@10.79.0.2
# Wi-Fi
ssh -NT -o ExitOnForwardFailure=yes -o HostKeyAlias="$zero_id" \
  -o StrictHostKeyChecking=yes -L 127.0.0.1:5944:127.0.0.1:5900 root@WIFI_IP
```

Point Remmina or another VNC viewer at `127.0.0.1:5943` for USB or
`127.0.0.1:5944` for Wi-Fi. Turn off the viewer's own SSH tunnel and choose
scaled mode for the fixed 1024×768 desktop. Sign in at the ANGI greeter with
the account you created. Run `agx tui` over SSH to make further network or
profile changes.

## Connect Agent Angi

Open **AI providers** in Agent Angi. For **OpenAI**, choose **Sign in with
ChatGPT** and enter the displayed code at the supplied URL in your workstation
browser. If your account requires device-code sign-in to be enabled, use its
security settings. For **LM Studio**, load a tool-capable model on your
workstation, start its server with **Serve on Local Network** enabled, and enter
the server's LAN address in ANGI. Select **Use** after the connection succeeds,
then choose models for Chat and Project.

In **Project**, select a folder you own and enter a brief. The agent opens in
the desktop terminal, where you can approve tool actions and follow progress.
Use **History** for the conversation or project summary and **Files** for the
resulting files. Chat, Project, and their account state remain available after
reboot.

## Hardware checks

Verify `/proc/device-tree/model`, the USB address, and `agx services --json`
after a cold boot. Check that Wi-Fi reconnects after reboot, that both VNC
tunnels reach the greeter and authenticated desktop, and that keyboard and
pointer input work. With VNC active, complete a Chat turn and a small Project
edit with each configured provider, then check `/proc/swaps`, memory usage,
and the kernel log for out-of-memory kills. For desktop startup failures,
inspect `/run/agx/gui.log` over USB SSH.
