# Phase 2 hardware and kernel audit

Audit of the running Raspberry Pi 5 development installation, November 15, 2026. Phase 2 remains in progress until the candidate kernel has been booted and its devices tested on the Pi.

## Observed on the Pi

- SD card `mmcblk0` is the mounted, writable root filesystem; the device reports 29.1 GiB. The current root partition is 256 MiB.
- USB gadget networking exposes `usb0` and key-authenticated recovery SSH at `10.77.0.2`. The kernel has DWC2 gadget and USB Ethernet built in, so they remain available before userspace networking starts.
- Broadcom BCM4345/6 Wi-Fi is present and connected through `brcmfmac`, `brcmutil`, `cfg80211`, and `rfkill`; firmware loaded successfully. DHCP, DNS, time sync, and saved connection recovery have already been tested.
- `eth0` is present; a physical Ethernet connection has not been tested. The 16 GB-class memory is detected. Local HDMI and UART recovery logins have not been physically verified.
- The boot log contains missing optional `txcap_blob` and channel-setting messages during scans. The current Wi-Fi connection works. `iw reg get` reports global region US and a device-specific region 99; investigate the firmware regulatory behavior, those messages, and supported channels before declaring the radio audit complete.
- The running kernel exposes 16 RAM block devices, while AGX boots directly from the SD filesystem and does not use them. The candidate configuration disables `CONFIG_BLK_DEV_RAM`.

## Candidate and acceptance

The kernel fragment preserves the existing built-in USB recovery settings and disables RAM block devices. The first candidate should be checked for successful build and image integrity, then physically booted before the change is accepted. Verify root SD read/write, USB SSH, saved Wi-Fi reconnection, DNS/HTTPS, the TUI and profile login, and kernel messages. Recheck the kernel configuration for the disabled option and confirm `/proc/partitions` no longer shows `ram0`–`ram15`. Keep the current working boot and recovery path available for rollback.

Further reduction requires an explicit device/feature audit. Keep display, keyboard, storage, Ethernet, Wi-Fi and serial support while recovery and future GUI requirements remain unresolved. Avoid inferring that an unloaded module is unnecessary solely from the present headless session.

The candidate image built and passed `make check-image`, including checks for the disabled RAM disks and retained USB gadget and Broadcom SDIO settings. `output/images/sdcard.img` SHA-256 at this checkpoint:

```text
6c73574454825e2265e795859459d2c3f1ceca873486c456f744b3d9e04b5fd6
```

This image has not been flashed or booted on the physical Pi. It is a fresh image, so writing it over the current card would replace its saved account and Wi-Fi settings; retain the working installation for recovery when planning the hardware test.
