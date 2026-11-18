# Wayland for ANGI

Goal: run Chromium and other native Wayland apps under an ANGI-owned compositor on the Pi 5.
The compositor owns DRM/KMS, input, focus, workspaces and window placement. Slint provides
the ANGI shell as a Wayland client, and ANGI Web retains its embedded WPE WebKit engine.

The direct DRM reference session below tests that architecture's display and client prerequisites
before replacing Weston's placement policy with ANGI's own. The shipped DRM/KMS desktop keeps the
private Cage and WayVNC integration for Chromium during development.

## Existing Chromium integration in the DRM/KMS desktop

ANGI owns the display: Slint draws straight to DRM/KMS and a tiling window manager (`wm.rs`) decides geometry.
A Wayland client needs a compositor to talk to. Rather than run a second compositor that would fight for DRM
master, ANGI hosts each application in a small **headless** compositor process and shows its output as a window,
the same way it already hosts WPE WebKit (`apps/web` plus `browser/`): the host renders offscreen, exports a
single-plane DMA-BUF, and the desktop imports it as a GPU texture and copies it once on the GPU.

```
 ANGI desktop (agx-gui, Slint on KMS)            agx-wl (one per app window)
 ┌─────────────────────────────┐   ANGIGPU1      ┌────────────────────────────────────┐
 │ window kind "Wayland app"   │◀─ dmabuf fd ────│ wlroots headless backend + GLES2   │
 │  imports dmabuf → texture   │   + sequence    │ xdg-shell, seat, dmabuf, data-dev  │
 │  forwards keys/pointer ─────┼── control ─────▶│ client (Chromium …) via WAYLAND_   │
 │  tiling/float/resize        │   (resize,ack)  │ DISPLAY=agx-wl-<id>                │
 └─────────────────────────────┘                 └────────────────────────────────────┘
```

Why one headless host per window rather than making `agx-gui` itself a compositor: it reuses the proven
Web bridge, keeps a misbehaving client from taking the desktop down, and lets the host be developed and tested
alone. The cost is one extra GPU copy per frame and one process per app. If graphical apps become central, a
single in-process compositor (Smithay) is the long-term alternative; the window kind, input and resize
plumbing built here carry over to it unchanged.

## Pieces

| Piece | Where | Notes |
| --- | --- | --- |
| Runtime libraries | `buildroot/configs/agx_wayland_defconfig`, `scripts/build-wayland.sh` | Separate build tree `output-wayland/`; the production image config is untouched. |
| Packaging | `scripts/package-wayland-runtime.py` | Ships the Wayland dependencies and a private versioned eudev library for Chromium. System libraries under `/lib` stay untouched. |
| Compositor host | `apps/wayland/` (`agx-wl`) | wlroots headless + GLES2; one toplevel, fullscreen to the output; DMA-BUF export; input injection. |
| Desktop side | `crates/agx-gui/src/wayland/` | Window kind, GPU import (generalizing `browser/gpu/`), input, resize. |
| Install path | app library (`apps/agent/apps.json`) | A new "Graphical" kind with a launch command; Chromium comes from GitHub like the CLI tools. |

## Phases and exit criteria

0. **Runtime builds.** `output-wayland/` builds wlroots, cage, foot and the 18 libraries Chromium's main
   binary needs. Exit: `readelf` shows no unresolved `NEEDED` for Chromium against image + runtime.
1. **Compositor host on the Pi.** `agx-wl` runs on the device with a Wayland test client (foot). Exit: a frame
   from `foot` arrives as a DMA-BUF that the host can export and the Pi's EGL can import.
2. **Desktop window.** A Wayland app window in the tiling desktop with keyboard, pointer, scroll, resize and
   close. Exit: foot usable as a terminal inside ANGI; resizing the window resizes the client without tearing.
3. **Chromium.** Launch ungoogled-chromium (GitHub aarch64 release) with `--ozone-platform=wayland`. Exit:
   loads a page with GPU acceleration; sandbox works (user namespaces are enabled on the device); measured
   frame rate and latency recorded against ANGI Web for the same page.
4. **Library integration.** Install/update/remove Chromium from the Apps tab like any other entry, with a
   Graphical kind that launches into a Wayland window instead of a terminal tab.
5. **Hardening.** Clipboard, cursor shapes, popups, fractional scale, multi-window apps, crash isolation.

## Risks and unknowns (to be settled by phases 1 and 3, not assumed)

- Whether Chromium's GPU process starts on the Pi 5's V3D driver inside a headless wlroots session.
- The headless backend has no vblank, so frame pacing is timer-driven; latency may need tuning.
- Single-plane DMA-BUF only: the desktop's importer rejects multi-plane or unusual modifiers.
- Chromium's size on the NVMe (about 700 MB per release, one previous version kept during an update).
- Root filesystem space: the runtime's links live on the SD root, which is about 75% full.

## Performance: what this does and does not buy

This architecture is about **compatibility** (running Chromium and other Wayland apps), not speed. Be explicit:

- It does **not** raise the desktop's own frame rate. That is Slint/femtovg on KMS and is unchanged. (The desktop's
  log already shows libinput warning that event processing lags by 22 to 60 ms at times; that is a separate GUI
  problem this work does not touch.)
- It **adds** cost relative to ANGI Web: one extra GPU copy per frame, a process hop, and a timer-driven frame pace
  because a headless output has no vblank.
- The most performant way to show a web page is the one ANGI Web already uses: render in-process to a single GPU
  buffer. Chromium behind agx-wl cannot beat that; the goal is to come close while supporting more sites and apps.

### YouTube and video

- ANGI Web **cannot play video or audio today**: multimedia (video, audio, WebRTC) is disabled in the WPE build
  (`apps/web/README.md`). Wayland does not change that. Enabling GStreamer in the WPE build is a separate, likely
  cheaper route to YouTube in ANGI Web, with no extra copy.
- The Pi 5 has **no H.264 hardware decoder** (only HEVC), and YouTube serves VP9/AV1/H.264, so playback is
  software decoding on the four A76 cores whichever browser is used. Reports of Chrome on a Pi 5 describe 1080p and
  1440p playing smoothly and 4K dropping frames.
- ungoogled-chromium is built without proprietary codecs (no H.264/AAC); YouTube falls back to VP9/AV1 and Opus.
- ANGI has no audio output configured yet (no ALSA libraries on the device).

Exit criterion for phase 3 therefore includes a measurement, not an assumption: dropped frames and CPU use playing
a 1080p60 VP9 YouTube video in Chromium under agx-wl, set against the same page in ANGI Web once media is enabled
there. `AGX_WL_TRACE=1` will log per-stage times.

## Architecture review: what Omarchy and Raspberry Pi OS actually do (2026-11-18)

- **Omarchy** (github.com/basecamp/omarchy) is the standard compositor-first design: Hyprland owns the display
  through its Aquamarine DRM backend and every app is a native Wayland client (Chromium launched with
  `--ozone-platform=wayland`; `GDK_BACKEND`, `QT_QPA_PLATFORM`, `MOZ_ENABLE_WAYLAND` forced to Wayland). Nothing
  unusual is done with Wayland. Blur and shadows are off by default, tearing is not allowed, and animations are
  turned off automatically for software rendering. It targets x86 desktops.
- **Hyprland on a Pi 5** has reports of a black screen with a recent Aquamarine (Arch Linux ARM forum, Nov 2024)
  and low frame rates on V3D (Gentoo forum, Jan 2024; old). It is not a Pi-proven choice.
- **Raspberry Pi OS** moved from Wayfire to **labwc**, a lean wlroots compositor, because it fit the Pi's graphics
  hardware better. In Tom's Hardware's Pi 5 YouTube tests, dropped frames at 1080p60 fell from 49.9% (Wayfire) to
  1.8% (labwc); at 4K60, from 82.5% to 60%. The compositor choice mattered more than the browser.

Consequence: the proven Pi recipe is a lean wlroots compositor with Chromium as a normal client, which is what
"ANGI as the compositor" means. The headless host-per-app design in this plan is an interim shortcut with more GPU
copies, and should not be built before the measurement below.

**Next gate (before writing agx-wl):** on the real Pi, run Chromium under `cage` on the actual DRM output (wlroots
direct scanout) and measure YouTube 1080p60 dropped frames and page scrolling, with the ANGI desktop stopped for
the test. That is the best-case baseline any ANGI compositor design has to match, and it uses exactly what
`output-wayland/` builds (wlroots, cage, Chromium's libraries).

## Status

- **Phase 0 done (2026-11-18):** `output-wayland/` builds in about 35 minutes (130 packages) with wlroots 0.20, cage,
  foot, wayland-info and every library Chromium's main binary needs. `package-wayland-runtime.py` produces
  `.cache/wayland-runtime.tar`: 148 files, **11.0 MB**, nothing that collides with the image (Mesa, libinput,
  libxkbcommon, libc and libudev are never shipped), and every dependency of every shipped file resolves against the
  runtime plus the image. All 18 directly linked libraries Chromium needs are covered.
- Next: the best-case measurement on the real Pi (Chromium under `cage`, ANGI stopped), then decide between an
  in-process ANGI compositor and the interim headless host.

## On the device (2026-11-18)

- **Runtime deployed** with `scripts/deploy-wayland.sh` (`--remove` undoes it exactly): release `fb73c757d9161c6e`,
  148 links, 10.8 MB on the NVMe under `/var/lib/agx-data/agx-wayland`. `cage`, `foot` and `wayland-info` resolve every
  library. Nothing the image already had was replaced.
- **Chromium installed** from the Apps tab (ungoogled-chromium 156.0.8078.79-1, 708 MB on the NVMe).
  Installing, removing and reinstalling it through the UI works.
- **Runtime updated** to include the NSS modules Chromium loads at runtime, `dbus-run-session`, and a
  private versioned eudev library. The supervised Bluetooth startup script makes the system D-Bus
  socket directory traversable for unprivileged clients. D-Bus policy still governs access to services.
- **Chromium render verified** on the Pi as the desktop user with `cage -- angi-chromium`, using Cage's
  headless backend with both Pixman and the V3D GLES2 renderer on `/dev/dri/renderD128`. Chromium loaded
  a test page and submitted a Wayland buffer with its default sandbox flags. This also exercised the
  launcher's session D-Bus fallback.

The first Chromium window uses Cage and WayVNC on a private Unix socket. The ANGI desktop displays
Chromium's framebuffer and routes keyboard, pointer and scroll input directly to it. The window scales
the compositor's fixed-size output to fit its assigned desktop tile. A DMA-BUF bridge, dynamic output
resizing and the direct DRM scanout and playback baseline remain future work.

Build with `scripts/build-wayland.sh` and deploy with `scripts/deploy-wayland.sh` after reading the
[Pi deployment guide](../docs/pi-ethernet.md). Use the Chromium entry in Apps or run `chromium`
from the signed-in user's terminal. Existing installs with a direct Chromium link can also use
`angi-chromium` or refresh the app's links during an install or update.

## Direct DRM reference session

Weston 15.0.1 was tested on the Pi 5 with its DRM backend on `card0` and the V3D GL
renderer on `renderD128`. The HDMI output was `HDMI-A-2` at 1920×1080. Weston's
scene graph showed ANGI (Slint with `SLINT_BACKEND=winit-femtovg`) and Chromium
(Ozone Wayland) as separate `xdg_toplevel` surfaces with separate process IDs.
Chromium loaded `https://example.com`. Weston enumerated the connected keyboard
and mouse using the test's eudev database.

The runtime ships Weston and seatd alongside the existing Wayland tools. The reference session
uses a private Mesa EGL front end built with the Wayland platform, linked against the same
V3D Gallium library as the appliance. Weston uses the runtime's real eudev library and a
temporary database of the devices present when the test starts. Hotplug and production seat
management are outside this check. The test restores the signed-in KMS desktop and removes its
temporary seat and device metadata when it exits.

From the workstation, run `make gui-wayland-build` for the Wayland-capable Slint binary.
Build and deploy the Weston runtime using the commands above. An existing Wayland output tree
from before Weston was enabled needs Buildroot's `seatd-reconfigure cairo-reconfigure` targets
so seatd includes its daemon and Cairo includes PNG support. With the SSH environment from the
Pi deployment guide loaded, stage `.cache/agx-gui-wayland` and
`scripts/wayland-drm-smoke.py` in a root-owned NVMe directory. While signed in to the normal
desktop, run the harness as root on the Pi:

```sh
python3 wayland-drm-smoke.py ./agx-gui-wayland ./evidence
```

The harness hands off the physical output using the desktop's existing refresh mechanism,
captures Weston's scene and HDMI framebuffer, and returns to the installed desktop. On the
standalone native Wayland build, the Apps launcher starts Chromium as a client of the same
compositor. The retained KMS desktop still opens Chromium through its private compositor.

## Native Chromium preview

The same harness accepts `--native --hold 120` to load ANGI's shell adapter and leave the
session available for two minutes after the capture. Build the adapter with
`scripts/build-native-wayland-shell.sh` after building Weston, or build the full runtime
with `scripts/build-wayland.sh`. Deploy that runtime and the Wayland-capable GUI as above.

In this preview, Weston owns DRM, composition and the input seat. ANGI's existing window
model controls Chromium's position, size, visibility and focus. The adapter configures the
Chromium `xdg_toplevel` to the entire tile and composites its buffer directly. Chromium draws
the one visible window frame and handles its close button. Its maximize and fullscreen
requests update the ANGI layout.
Chromium receives Wayland pointer and keyboard events from Weston. The shell and browser
remain separate surfaces. The ANGI Web panel continues to use WPE WebKit.

The preview supports one Chromium toplevel on the primary HDMI output. An ANGI panel that
overlaps it from above temporarily hides the browser surface. Keeping portions visible under
overlapping panels, placing additional native toplevels and tracking hardware hotplug are
future work. The appliance still boots into the current KMS desktop. A Smithay backend can replace the
Weston adapter and share ANGI's layout policy as external surface support grows.

### Live native session with VNC

`scripts/build-weston-vnc.sh` adds Weston's VNC backend to the Wayland runtime build.
It mirrors the HDMI output at the HDMI mode's resolution, with Weston routing remote
input to the Wayland surface under the pointer. The only listener is
`127.0.0.1:5900` on the Pi, through the established OpenSSH tunnel at `127.0.0.1:5941`.

After staging the Wayland-capable GUI and harness on the Pi, run
`python3 wayland-drm-smoke.py ./agx-gui-wayland ./native-session --session` as root
from the staging directory. It hands off the signed-in session, then stays active until
terminated or until the shell exits. Start it in a detached process to keep it running
after closing SSH. To return to the usual desktop, terminate the harness PID recorded in
`native-session/session.pid`. Its cleanup restores the previous signed-in desktop.

