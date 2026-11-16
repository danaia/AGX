# AGX desktop (GUI)

`agx-gui` is a Slint front end with Omarchy/Hyprland-style window management. The tiling engine is `crates/agx-gui/src/wm.rs`, and the key bindings are `keymap.rs`. Both are unit-tested (`scripts/cargo.sh test --manifest-path crates/agx-gui/Cargo.toml`).

Run a preview window on a desktop: `scripts/cargo.sh run --manifest-path crates/agx-gui/Cargo.toml`.

## Boot and graphical login

With `/usr/bin/agx-gui` installed, Rust init starts `agx-gui --login` directly on HDMI (`tty1`). There is no text username/password prompt or interface chooser. Sign in with the existing AGX administrator username and password; the desktop runs as that user, with their home directory and supplementary groups. Root, empty passwords, locked accounts and expired credentials are rejected. A failed login has a two-second retry delay. Exiting the desktop or the TUI session returns to graphical login through the supervisor.

`agx desktop` opens the desktop immediately. Its **Open TUI** menu action switches to the authenticated user's TUI, and **Sign out** returns to login. SSH and serial (`ttyAMA10`) retain their existing login paths. Images without the GUI retain the HDMI text login. A GUI startup failure is retried with supervisor backoff; recovery is available through SSH or serial.

Install the GUI, CLI, init and device-access helper with `make deploy-gui`, then reboot. Override `AGX_SSH_HOST` and `AGX_SSH_KEY` when needed. The running PID 1 stays in place until reboot. Original installed binaries are saved in `/root/agx-gui-backup`. The GUI is still built/deployed separately from the Buildroot Rust workspace; a fresh image needs the GUI binary and desktop runtime libraries installed before graphical login is selected.

Preview just the login screen without root or real authentication:
`scripts/cargo.sh run --manifest-path crates/agx-gui/Cargo.toml -- --login-preview`.

## Modifier

On the appliance (`--features kms`, no compositor) the modifier is **Super**, as in Omarchy. In a desktop preview Super belongs to the compositor, so the default is **Alt**. Override it with `AGX_GUI_MOD=super|alt`. Below, `Mod` means that key. "A little" is Alt with Super (Ctrl with Alt); "a lot" is Ctrl with Super (Super with Alt).

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
| Mod+L | Window mode: float every window in place, with title bars; press again to tile |
| Mod+1..5 | Switch workspace (plain 1..5 also works) |
| Mod+Shift+1..5 | Move the window to a workspace and follow it; add "a little" to stay |
| Mod+Tab, Mod+Shift+Tab | Next, previous workspace |
| Alt+Tab | Cycle window focus |
| Mod+`-` and Mod+`=` | Narrow or widen by 100px (Shift: height) |
| a little / a lot | 25px or 300px steps |
| Mod+Space or `/` | Menu: workspaces, closed panels, window actions, TUI, reboot, shut down |

## Mouse

- Click a window to focus it; click a workspace number to switch.
- Drag a title bar to move a window. A tiled window shows a ghost and swaps with the tile it is dropped on; a floating window moves freely.
- Drag the gap between two tiles to resize the split. Drag the edges or corners of a floating window to resize it.
- Mod+left-drag moves a window from anywhere in it; Mod+right-drag resizes it. Mod+scroll changes workspace.
- Title bar buttons: float or tile, fullscreen, close. Right-click a title bar for the menu.

## Layout

Dwindle tiling: a new window splits the focused tile along its longer side. Gaps are 10px at the screen edge and 10px (2 × 5) between tiles, with 2px borders and square corners, matching Omarchy's Hyprland defaults. The focused window has the cyan-to-green border.

Not implemented: window grouping (tabs), scratchpad, multiple monitors.
