# ANGI desktop (GUI)

`agx-gui` is a Slint front end with Omarchy/Hyprland-style window management. The tiling engine is `crates/agx-gui/src/wm.rs`, and the key bindings are `keymap.rs`. Both are unit-tested (`scripts/cargo.sh test --manifest-path crates/agx-gui/Cargo.toml`).

The desktop starts in **windowed** mode: Settings fills the left half, with the ANGI overview above CPU and Memory and storage on the right. The overview uses `brand/brand-mark-text.svg`. All five workspaces start in windowed mode; click **windowed** or press **Super+L** to switch to tiling. Windows still at their initial positions follow display size changes; moved or resized windows keep their custom geometry.

Run a preview window on a desktop: `scripts/cargo.sh run --manifest-path crates/agx-gui/Cargo.toml`.

## Boot and graphical login

With `/usr/bin/agx-gui` installed, Rust init starts `agx-gui --login` directly on HDMI (`tty1`). There is no text username/password prompt or interface chooser. Sign in with the existing AGX administrator username and password; the desktop runs as that user, with their home directory and supplementary groups. Root, empty passwords, locked accounts and expired credentials are rejected. A failed login has a two-second retry delay. Exiting the desktop or the TUI session returns to graphical login through the supervisor.

`agx desktop` opens the desktop immediately. Its **Open TUI** menu action switches to the authenticated user's TUI, and **Sign out** returns to login. SSH and serial (`ttyAMA10`) retain their existing login paths. Images without the GUI retain the HDMI text login. A GUI startup failure is retried with supervisor backoff; recovery is available through SSH or serial.

The power icon at the right of the GUI header opens a session dropdown: **Log out**, **Enter TUI**, **Reboot**, and **Shut down**. Use the mouse or Up/Down and Enter; Escape or clicking outside dismisses it. Reboot and shutdown require confirmation, with clickable Confirm/Cancel buttons.

From the HDMI TUI, **F8** returns directly to the GUI. The same action is available as **Back to GUI** in Profile and **[GUI F8]** in the footer beside shutdown and reboot. Save or discard Profile edits before switching. Both directions run through one authenticated CLI session; repeated switching does not create nested launchers. SSH/serial TUI sessions display the shortcut disabled and stay in their terminal rather than opening the Pi's graphical display.

Install the GUI, CLI, init and device-access helper with `make deploy-gui`, then reboot. Override `AGX_SSH_HOST` and `AGX_SSH_KEY` when needed. The running PID 1 stays in place until reboot. Original installed binaries are saved in `/root/agx-gui-backup`. The GUI is still built/deployed separately from the Buildroot Rust workspace; a fresh image needs the GUI binary and desktop runtime libraries installed before graphical login is selected.

The development Pi is reachable over Wi-Fi at `192.168.1.184` (DHCP; checked November 16, 2026). Its SSH host key matches the recorded USB address. To deploy over this connection:

```sh
AGX_SSH_HOST=root@192.168.1.184 AGX_SSH_HOST_KEY_ALIAS=10.77.0.2 make deploy-gui
```

Wi-Fi deployment requires the NVMe volume to be mounted. It stores the GUI binary, Mesa/runtime files and original binary backups under `/var/lib/agx-data/agx-gui`, with symlinks at their normal system paths. This avoids filling the small SD root filesystem. Libraries are replaced by symlink rename, preserving mappings held by running processes; deployment does not replace libc or its loader.

The data-volume mount script `/etc/init.d/S12agx-data` must remain a regular file on SD. An older OpenCode deployment linked it into its NVMe release, preventing the volume from mounting on reboot and selecting the HDMI text recovery login. To repair that installation over root SSH, mount the configured data volume after verifying its UUID against `/etc/agx/data-volume.uuid`, copy the resolved script to a new regular file on SD, and atomically replace the symlink with that file. Reboot to restart graphical login. The OpenCode installer now copies this boot script instead of linking it.

To install graphical login directly onto an SD card connected to the workstation, run `sudo scripts/install-gui-login-sd.sh /path/to/agx-rootfs /path/to/agx-boot`. This places the GUI executable on SD, verifies the Rust PID 1 boot entry, enables the existing graphical greeter, and checks that accounts and Wi-Fi settings remain unchanged. The Pi's previously installed Mesa/input libraries still require its NVMe volume. A missing GUI executable selects the HDMI text recovery login; missing GPU libraries cause GUI startup retries while SSH/serial recovery remain available.

Both physical Super keys drive desktop shortcuts on HDMI. Slint 1.18's LinuxKMS mapping omits `Super_L`/`Super_R`, so ANGI uses its public libinput hook to deliver those two keys as Slint Meta press/release events to the current window. Other keys keep Slint's existing layout and modifier handling; VNC retains its separate keysym mapping. The hook feature is pinned to the installed Slint/input API and should be reviewed when upgrading Slint. Native scrolling uses the same bridge: wheel v120 units are converted to 60 logical pixels per notch, fractional high-resolution steps are retained, and touchpad/continuous scroll uses logical pixel deltas. Both axes follow Slint/VNC direction conventions. The deprecated duplicate axis event is consumed without scrolling twice. Native cursor position is tracked so the hovered page or scrollable UI receives the event.

Local keyboards and mice are discovered through libinput/libudev-zero. The supervised display-access service runs `agx-display-devices`, which grants `agx-admin` access before forwarding kernel input arrival/removal notifications to libudev-zero. This covers keyboards that enumerate late during boot and devices reconnected through a USB hub or switch. It relays device lifecycle notifications, not keypress contents. Install the helper with the GUI; the older permission-only shell fallback cannot notify libinput of newly attached devices.

Preview just the login screen without root or real authentication:
`scripts/cargo.sh run --manifest-path crates/agx-gui/Cargo.toml -- --login-preview`.

## Mouse settings

Open the top-right power menu and choose **Settings**, or search for Settings in Apps. Settings opens as a normal desktop tile alongside your other windows. It supports tiling, floating, resizing, fullscreen and workspace moves; opening it again focuses the existing tile. The Mouse & pointer panel offers pointer speed, adaptive acceleration, left/right primary button, natural scrolling and scroll speed. Changes apply immediately to connected native pointing devices and are reapplied when a mouse reconnects. The test area lets you try movement and clicks; **Restore defaults** resets this collection of preferences. Close it with the title-bar close button or the normal desktop shortcut; Escape leaves the tile open. The category column collapses to a narrow icon rail in small tiles, and controls stack below their descriptions. **Refresh desktop…** at the bottom asks for confirmation, saves pending preferences, closes the desktop’s terminal and browser sessions, and starts the latest installed GUI as the same signed-in account. It does not reboot the Pi; VNC viewers may need to reconnect.

Preferences belong to the signed-in account and are saved atomically in `$XDG_CONFIG_HOME/agx/mouse.conf` (normally `~/.config/agx/mouse.conf`). The greeter keeps its own defaults. Native controls apply through libinput; VNC movement remains controlled by the viewer. Desktop previews show the controls without changing the workstation's mouse settings. Device capabilities determine which native options the hardware supports.

## Rendering and performance

Both appliance and preview builds include FemtoVG and software rendering. The default on the Pi is `auto`: a connected HDMI monitor selects `linuxkms-femtovg`; otherwise it selects the resizable `vnc-software` display, which requires VNC to be enabled. Previews default to `winit-femtovg`. Mesa provides V3D, VC4, EGL, GLES and GBM; the kernel's `v3d` and `vc4` modules provide rendering and HDMI respectively. No X11 or Wayland server is needed on the Pi. Renderer selection is explicit: initialization failures are reported rather than silently producing software-renderer benchmark results. Slint draws the normal GPU pointer. LinuxKMS maps built-in resize shapes to its default arrow, so AGX overlays directional move/resize images and hides the native pointer while those shapes are active. The software renderer uses the same images in its cursor overlay. Cursor strokes use rounded tips and switch between pearl/dark-halo and slate/white-halo variants according to the current background palette. Run `scripts/cargo.sh run --manifest-path crates/agx-gui/Cargo.toml -- --cursor-preview` for the live dark/light comparison at 1× and 3× scale.

Pointer hover uses the cached window scene without rebuilding layout or replacing Slint models. Tiled dragging updates only the ghost and affected target highlights. Floating dragging and resizing notify changed window rows in a persistent `VecModel`. Membership/stacking changes rebuild the rows within that same model. Motion remains immediate; frame-based input coalescing can be evaluated if physical profiling still shows a backlog.

The root greeter reads `/etc/agx/gui.env` and passes its graphics options to the authenticated session. Environment variables override the file. It accepts only these literal `KEY=value` options (no shell expressions):

```text
SLINT_BACKEND=linuxkms-femtovg
# Optional while measuring frame rate; leave off for normal use.
# SLINT_DEBUG_PERFORMANCE=refresh_lazy,console
# Optional: select the connected output and its listed 1080p/60 mode index.
# SLINT_DRM_OUTPUT=HDMI-A-1
# SLINT_DRM_MODE=4
# Remote desktop over VNC; see "Remote access (VNC)" below.
# AGX_GUI_VNC=on
```

Set `SLINT_DRM_MODE=list` to list actual supported modes in the log, then replace `list` with the index for **1920×1080 at 60 Hz**. Mode indices depend on the connected display; `4` above is only an example. Reboot after changing the configuration to restart the protected graphical console cleanly. Greeter and authenticated-session stderr are written to `/run/agx/gui.log`, readable through root recovery SSH. This log is volatile and rotates between sessions after 4 MiB. Remove the performance option after measuring.

Compare `linuxkms-software` and `linuxkms-femtovg` at the same mode. Drag floating windows, resize splits, and switch workspaces. Use `refresh_lazy` during continuous motion, record FPS dips as well as sustained rates, and aim for 60 frames/s (16.7 ms/frame). Idle lazy-refresh FPS is not a performance failure. Repeat at 4K only after the 1080p baseline. FPS averages alone do not establish worst-case frame latency. See [Slint's LinuxKMS backend](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_linuxkms/) and [performance debugging](https://docs.slint.dev/latest/docs/slint/guide/development/debugging_techniques/).

For a headless GPU check, cross-build `scripts/check-gpu.c` against the Buildroot EGL/GLES libraries and run it on the Pi. It creates a GLES2 context and requires the renderer string to identify V3D. This verifies driver/context initialization, not HDMI performance.

On November 16, 2026, the headless check passed as root and `dana`: EGL 1.5, Broadcom V3D 7.1.10.2, OpenGL ES 3.1, Mesa 26.1.8. Both HDMI ports were disconnected, so HDMI startup and frame-rate measurements remain unverified.

The first GPU deployment mistakenly linked `/lib/libgcc_s.so.1` to NVMe, creating a boot dependency cycle; the Pi did not return after reboot. The packager now excludes `/lib` and `/lib64`, and deployment checks that PID 1's resolved dependencies do not require NVMe. To repair that first installation, power off the Pi, mount its SD root partition on the workstation, then run `sudo scripts/repair-gui-boot.sh /path/to/mounted/agx-rootfs`. This restores only the original ARM64 `libgcc_s.so.1` to the SD filesystem. Unmount the card before returning it to the Pi. Boot recovery still needs confirmation after this repair.

## Remote access (VNC)

`agx-gui` can serve its own screen over VNC, so a viewer such as Remmina can see and drive the desktop and the login screen without a monitor on the Pi. It is off by default. Set `AGX_GUI_VNC` to turn it on: put `AGX_GUI_VNC=on` in `/etc/agx/gui.env` on the Pi and reboot (the greeter passes it to the signed-in session), or export it when running a preview. Accepted values are `on` (127.0.0.1, port 5900), a port number, or a loopback `address:port`; `off` or unset disables it. The log in `/run/agx/gui.log` shows `vnc listening on ...` when it started.

For an actual desktop resolution that follows the Remmina window, select the VNC-only software display in `/etc/agx/gui.env`:

```text
SLINT_BACKEND=vnc-software
AGX_GUI_VNC=on
```

Restart the graphical session (or reboot), then use Remmina's **Toggle dynamic resolution update** mode; saved profiles use `scale=2`. This changes the rendered workspace dimensions rather than stretching a fixed-size image. The initial display is 1280×720; resizing Remmina requests its available content dimensions, excluding the toolbar and window decorations. One screen is supported, from 320×200 through 4096×2160; the UI is most useful at 640×400 or larger. All viewers share the desktop size, so the latest accepted request applies to everyone. Screen layout capability and the current viewer size survive login handoff. LibVNCClient 0.9.15 ignores screen ID zero and sends uninitialised screen metadata when resizing. ANGI therefore canonicalises the single-screen metadata to ID 1 at origin (0,0), with no flags, while using the requested dimensions; this compatibility behaviour differs from strictly echoing the client's metadata in a successful reply. Invalid sizes, multiple screens, or screen dimensions that differ from the framebuffer receive a failure reply without allocating a new capture.

`vnc-software` renders only for remote viewers and does not draw the desktop on HDMI or read local keyboard/mouse input. SSH and serial recovery remain available. To restore the local display, select `linuxkms-software` or `linuxkms-femtovg` and restart the graphical session. With those fixed HDMI backends, VNC continues to mirror the physical display; client resize requests receive a refusal, and Remmina's scaled mode (`scale=1`) can fit the picture into its window.

For a complete PC with a local HDMI screen and optional VNC mirroring, use:

```text
SLINT_BACKEND=auto
AGX_GUI_VNC=on
```

Automatic mode checks both HDMI connectors for up to two seconds at graphical startup. With HDMI connected, VNC mirrors the monitor; without HDMI, Remmina can change the desktop resolution dynamically. The selected backend is retained across sign-in. Selection happens at startup, including after logout/restart; plugging in a monitor during a running VNC-only session requires restarting the graphical session. Explicit backend settings still override detection.

Remove old `video=HDMI-A-1:...D` overrides from the boot command line: those force HDMI to appear connected with no cable. Automatic detection ignores a forced connector unless it also has monitor EDID evidence. Normal connector detection does not require EDID.

Verified on November 16, 2026: after removing the forced-HDMI boot argument and rebooting the development Pi with both ports disconnected, automatic mode selected `vnc-software` and started the VNC login screen. The GUI suite passes 90 tests, including both HDMI ports, requested-output filtering and forced connectors without monitor evidence.

A working VNC login with a blank HDMI screen can indicate that `vnc-software` is still selected. Check `/etc/agx/gui.env` and `/run/agx/gui.log` before changing HDMI boot settings. Restart the graphical session after selecting the physical backend; the configuration persists across boots. Use Remmina's scaled mode for fitting this mirrored desktop into its window.

Verified on the development Pi on November 16, 2026: scripted resize requests produced full frames at 1600×900, 1920×1080, 1024×768 and 1280×720, with keyboard events and oversized-request rejection. Remmina 1.4.43 with LibVNCClient 0.9.15 changed the rendered size from 1454×861 to 1554×911 during window resizing. After deployment and reboot, the main Ethernet profile negotiated 1215×991 for its usable content area; the firmware power check returned `throttled=0x00000000`. Both saved ANGI profiles use `scale=2`. The previous Pi executable link and graphics configuration are saved under `/root/agx-vnc-dynamic-backup/`. The workspace suite and all 69 standalone GUI tests passed; resize capability across login handoff is covered by a live socket test.

The server listens on **loopback only** and offers no VNC password, because the classic VNC password scheme (a DES challenge) is too weak to put on a network. Access control is SSH: a non-loopback address is refused, and you connect through an SSH tunnel. Anyone who can already open a TCP connection to the Pi's loopback port, such as another logged-in user, can type into the desktop, so enable it only on a Pi whose shell users you trust.

Use an OpenSSH tunnel, not Remmina's built-in one: Remmina's libssh tunnel crashed Remmina (SIGSEGV inside libssh while closing a tunnel) on November 16, 2026. `scripts/vnc-tunnel.sh` runs the tunnel with `ssh` instead, listening on `127.0.0.1:5941` (Ethernet, to `10.78.0.2`) or `127.0.0.1:5942` (Wi-Fi, to `192.168.1.184`). Start one with `scripts/vnc-tunnel.sh up ethernet` (or `wifi`), check it with `status`, stop it with `down`. To keep both up automatically, including after a Pi reboot, install the user service from the repository root. Stop any manually started tunnels first so they do not occupy the service ports:

```bash
scripts/vnc-tunnel.sh down ethernet
scripts/vnc-tunnel.sh down wifi
install -Dm644 scripts/systemd/angi-vnc-tunnel@.service ~/.config/systemd/user/angi-vnc-tunnel@.service
systemctl --user daemon-reload
systemctl --user enable --now angi-vnc-tunnel@ethernet angi-vnc-tunnel@wifi
```

Once enabled, the script's `up` and `down` commands start and stop the corresponding service; systemd restarts unexpected SSH exits every three seconds. Keep this checkout at `~/dev/AGX`, or adjust the unit's `ExecStart`. User services start at login; with user lingering enabled they start at workstation boot and stay available after logout (`loginctl show-user "$USER" -p Linger` checks it). Lingering is already enabled on the development workstation. If the Pi is off when the workstation starts, SSH retries until the Pi returns. Existing VNC sessions may disconnect when the network drops; reopen the saved Remmina profile once the tunnel returns.

In Remmina, create a profile with protocol **VNC**, **Server** `127.0.0.1:5941` (or `5942`), the SSH tunnel option **off**, and no VNC password. Remmina's "Execute a Command" before-connect setting did not run in this version, so do not rely on it to start the tunnel. Adjust the Pi's address, key and user with `AGX_ETHERNET_HOST`, `AGX_WIFI_HOST`, `AGX_SSH_KEY` and `AGX_SSH_USER`. ANGI sends uncompressed pixels, so on slow Wi-Fi choose a lower colour depth in Remmina to reduce traffic.

The VNC server claims each update request before sending its frame, preserving another request received during transmission. Clearing the request after the write can lose the viewer's next request and leave the picture frozen while SSH stays connected. A live socket regression test reproduces that overlap; it failed before the fix and passes afterward. On November 16, 2026, the same overlapping-request probe stalled on the development Pi before deployment; after deploying the fix and restarting the graphical login, Ethernet delivered all 20 full-screen frames with Shift press/release events in 2.55 s without losing a request. The user subsequently confirmed that typing in Remmina over Ethernet was fast and stable.

The TUI's **Network** and **Services** pages show observed VNC listening addresses, established TCP connection count, and GUI resident memory (RSS, including VNC). Taller terminals also show the Pi's IPv4 addresses and the SSH-tunnel requirement; compact terminals retain the VNC and memory lines. The same information is in `agx status --json` under `vnc`. The monitor uses `AGX_GUI_VNC` from the session or readable `/etc/agx/gui.env` to select the port, otherwise 5900. Socket connections are counted before the VNC handshake, so the count is not an authenticated-viewer count. VNC runs inside the GUI process, rather than as a separate service with its own restart button. The workstation's tunnel services belong to its systemd user manager and are not Pi services. Switching the graphical session to the TUI stops VNC until the GUI is opened again; an SSH TUI can inspect it while the GUI stays running.

VNC retains a shared RGBA capture and each viewer's previous capture for change detection. At 1920×1080, each full capture is about 7.9 MiB. Pixel conversion now streams one row through a 64 KiB output buffer instead of allocating a second full encoded frame per viewer. The handshake also keeps only the screen dimensions, so it no longer pins the first capture for the entire session. The shared capture is released after the last viewer disconnects. This reduces live allocations without changing resolution, pixel formats or encoding; allocator caching means RSS may not immediately fall by the same amount. GUI RSS includes rendering and GUI data as well as VNC, and system-wide used memory includes other processes and kernel allocations. The TUI calculates used memory as total minus `MemAvailable`; BusyBox `free` uses a different calculation, so their used totals can differ.

Signing in replaces the root login process with your desktop, which would normally drop every connection. The login screen therefore passes its listening socket and connected viewers to the desktop through the exec (`AGX_GUI_VNC_HANDOFF`, with `agx` closing its own copies), and the desktop carries on serving the same connection, sending a full picture as soon as it has one. If anything about the handoff looks wrong the desktop ignores it and opens its own listener, and then the viewer shows a dropped connection and you reconnect (Remmina's reconnect button, or open the profile again).

How it works: the UI thread captures the window with Slint's `take_snapshot` only while a viewer is waiting (about 2 frames/s idle, about 15 frames/s for two seconds after input), sends just the changed rectangle in Raw encoding, and resizes the viewer if the window size changes. Pointer and key events are injected as Slint window events, so the shortcuts below work remotely. Automatic mode uses the independent software display when no HDMI monitor is present; it does not need a forced HDMI boot mode. Explicit KMS backends require a connected display and log startup failures in `/run/agx/gui.log`. The pointer you see is the viewer's local cursor. On the Pi the Mod key is Super, which many viewer desktops intercept; plain `1`..`5` switch workspaces and `/` opens the menu without it.

Status: the protocol, input mapping and server have unit tests (including a live socket client). On November 16, 2026, Remmina displayed the desktop preview window on the workstation, including a window resize, and a scripted client confirmed keyboard input, incremental updates, and 32-bit and 16-bit pixel formats. On the development Pi (no monitor, historical forced-connector configuration, `linuxkms-femtovg`) a client reached the graphical login screen through an SSH tunnel to Dropbear and received a 1920×1080 frame; the first full frame took about 2.8 s over Wi-Fi. After signing in, a reconnect from Remmina's saved profile showed the live desktop. Over Wi-Fi the first full frame took about 2.5 s and later updates 0.5-1 s each, because pixels are sent uncompressed; that is the main thing to improve. The sign-in handoff is covered by unit tests (a new server adopting a listener and a connected viewer, resizing it, and accepting new ones) but was deployed to the Pi only on November 16, 2026 and still needs confirming with a real sign-in over VNC. Not implemented: compressed encodings (ZRLE, Tight), clipboard sharing, a VNC password, and multi-screen resize layouts. The VNC-only software display described above works without a forced connector.

### Tracing a GUI freeze

Run `python3 scripts/trace-gui.py` on the workstation before reproducing a freeze. It saves logs under `.cache/gui-traces/` for two hours, reconnecting after Pi restarts. The default destination is `root@10.78.0.2`, using the existing SSH key and the USB host-key alias; override these with `--host`, `--key`, and `--host-key-alias`. Use `--duration SECONDS` to change the capture lifetime, or Ctrl+C to stop it. No Pi binaries or settings are changed.

The capture includes GUI stderr, kernel messages, syslog, the service lifecycle journal, memory, temperature, boot IDs, and GUI thread kernel stacks sampled every half second. Separate SSH streams prevent a blocked stack read from stopping the health logger. Workstation timestamps remain useful even while the Pi's clock is unset. Workstation kernel messages and tunnel logs distinguish a lost Ethernet carrier from a GUI connection closing. Each stream rotates at 8 MiB with three previous files retained. Existing application logs are included; screen pixels and process memory are not captured.

The Pi's normal GUI and system logs are volatile, so start the collector before reproducing the failure. If the GUI freezes, leave the Pi powered on briefly to allow stack capture. A final GPU fence wait is evidence of in-flight graphics work, not proof that the GPU caused the failure; correlate it with kernel errors, process exits, boot IDs, and the timing of any manual restart.

## Direct Ethernet

A cable straight between the workstation and the Pi's Ethernet port gives a fast, private link that needs no router or Wi-Fi. The Pi uses `10.78.0.2/24` on `eth0` (set in `/etc/network/interfaces`; the original is saved as `interfaces.pre-eth0`), and the workstation uses `10.78.0.1/24`, for example with `nmcli con add type ethernet con-name agx-ethernet ifname enp1s0 ipv4.method manual ipv4.addresses 10.78.0.1/24 ipv4.never-default yes ipv6.method disabled connection.autoconnect no`, then `nmcli con up agx-ethernet`. Setting `connection.autoconnect yes connection.autoconnect-priority 10` on it brings it up whenever the cable returns, for example after a Pi reboot; the cost is that plugging that port into a router would keep the static address, so switch with `nmcli con up "Wired connection 1"` or turn autoconnect off. A manual `nmcli con down` blocks auto-start until the next `con up`, and NetworkManager then activates the DHCP profile instead. This subnet is separate from the USB one (`10.77.0.0/24`). Connect with `ssh -i ~/.ssh/id_ed25519 root@10.78.0.2`; VNC works the same way through that tunnel. On November 16, 2026, this link negotiated 1000 Mbps, answered ping in about 0.2 ms, and delivered the first full VNC frame in 0.09 s, against about 2.5 s over Wi-Fi. A static address means `eth0` will not get a DHCP address if you later plug it into a router.

## Modifier

The main modifier is **Super** in both appliance and preview builds. **Super+Space** opens Applications. For a nested preview whose host compositor reserves Super, explicitly set `AGX_GUI_MOD=alt`; `AGX_GUI_MOD=super` selects the normal default. Below, `Mod` means that key. "A little" is Alt with Super (Ctrl with Alt); "a lot" is Ctrl with Super (Super with Alt).

## Applications launcher

**Super+Space** opens an app-first launcher. Apps lists the system tools, Web, native Terminal and System Console, including running tools; selecting a running tool focuses it or brings it to the current workspace. Each app has an embedded vector icon, a description and a running indicator. **Commands** holds window actions, workspaces and session controls. Power actions retain their confirmation step.

Type to search across apps and commands. Search recognises terms such as Wi-Fi, RAM, disk and reboot. Up/Down select a result, Page Up/Page Down move five results, Enter opens it, Tab switches Apps/Commands, and Escape closes the launcher. Mouse selection and scrolling work throughout the clipped results area. The panel adapts to the window height and shortens when search returns fewer results; unmatched queries show a useful no-results state. `/` remains a fallback shortcut when a VNC viewer intercepts Super. In Remmina, enable **Grab all keyboard events** to send Super shortcuts to ANGI while the viewer is focused.

On November 16, 2026, the launcher passed visual checks at 1280×900, 720×700 and 640×400, including app search, command scrolling, empty results, mouse activation and reaching the last app by keyboard in a small window. The Pi preview confirmed Super+Space, Wi-Fi search, opening Network and small-window navigation. The workspace tests and 71 GUI tests passed. The tested executable is installed on the development Pi, and both saved ANGI Remmina profiles enable keyboard grab; Right Ctrl twice toggles the grab with the workstation's current Remmina shortcuts. The previous executable link is saved under `/root/agx-launcher-backup/`.

## Keyboard (same chords as Omarchy's `bindings/tiling.lua`)

| Keys | Action |
| --- | --- |
| Mod+Arrows | Focus the window in that direction |
| Mod+Shift+Arrows | Swap with the window in that direction |
| Mod+Q or Mod+W | Close the window (reopen it from the menu) |
| Mod+J | Toggle the split direction |
| Mod+T | Float or tile the window |
| Mod+F | Fullscreen (bars hidden) |
| Mod+(a little)+F | Maximize (full width) |
| Mod+L | Toggle tiled/windowed mode; the header's layout label is also clickable |
| Mod+1..5 | Switch workspace (plain 1..5 also works) |
| Mod+Shift+1..5 | Move the window to a workspace and follow it; add "a little" to stay |
| Mod+Tab, Mod+Shift+Tab | Next, previous workspace |
| Alt+Tab | Cycle window focus |
| Mod+`-` and Mod+`=` | Narrow or widen by 100px (Shift: height) |
| a little / a lot | 25px or 300px steps |
| Super+Space (`Mod+Space` with an explicit override), or `/` | Applications launcher; Apps first, desktop/session actions under Commands |

## Mouse

Click **tiled** / **windowed** in the top bar to switch layout. Windowed mode lets you freely move windows by their title bars and resize their edges. The first switch preserves the current tiled positions; later switches restore your previous floating positions. Double-click a floating window's title bar to collapse its body into a movable header, then double-click again to restore its full size at the new position. A drag does not count as a double-click. Returning to tiled mode expands collapsed windows, and opening a collapsed app from Applications expands it too. Display resizing keeps floating title bars reachable.

Verified on November 16, 2026 in both the software preview and on the Pi over VNC: clicking the layout label, collapsing to a 30-pixel header, moving that header, restoring the full body, returning to tiled, and restoring the saved floating position all passed. The workspace suite and 77 GUI tests passed, including double-click timing, drag rejection, collapse hit testing and keeping headers reachable after a display resize. The development Pi runs the tested executable; the previous executable link is retained under `/root/agx-windowed-backup/`.

- Click a window to focus it; click a workspace number to switch.
- Drag a title bar to move a window. A tiled window shows a ghost and swaps with the tile it is dropped on; a floating window moves freely.
- Drag the gap between two tiles to resize the split. Drag the edges or corners of a floating window to resize it.
- Mod+left-drag moves a window from anywhere in it; Mod+right-drag resizes it. Mod+scroll changes workspace.
- Title bar buttons: float or tile, fullscreen, close. Right-click a title bar for the menu.

## Layout

Dwindle tiling: a new window splits the focused tile along its longer side. Gaps are 10px at the screen edge and 10px (2 × 5) between tiles, with 2px borders and square corners, matching Omarchy's Hyprland defaults. The focused window has the cyan-to-green border.

Not implemented: window grouping (tabs), scratchpad, multiple monitors.

Native **Terminal** opens with **Super+Enter** or from Apps. Its Alacritty engine provides shell emulation inside the existing window system, with Bash, tabs, searchable scrollback and CLI tools. See [Terminal](terminal.md) for shortcuts, runtime packaging and validation.
