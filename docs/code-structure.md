# Code structure and file-size review

Codex reads the repository's `AGENTS.md`; Claude is directed to the same instructions by `CLAUDE.md`. These rules apply to work in this repository, including IDE sessions.

## Review thresholds

Line counts are review signals, not architectural goals. Use roughly 500 lines to trigger an evaluation and roughly 750 lines to trigger segmentation when a file contains separable responsibilities. Library entry points should normally stay below 150 lines. Evaluate large executable entry points too: they should wire modules together rather than own UI state, rendering, and input handling.

Before extending a file at these thresholds, identify what it owns and which responsibilities can be moved behind a stable interface. A cohesive algorithm, generated file, protocol definition, or self-contained test fixture can justify a larger file. Record that reasoning in the change summary; do not split tightly coupled logic into arbitrary pieces just to reduce its line count. Generated/vendor/build outputs are not hand-refactoring targets.

Existing large files should be evaluated when touched, rather than forcing unrelated cleanup into every change. `python3 scripts/check-file-sizes.py` reports the current review backlog without failing a build merely because existing files are large.

## How to segment

1. Separate actual responsibilities: public models, parsing/validation, transport, application state, rendering, input dispatch, terminal/platform lifecycle, and background collection.
2. Keep `lib.rs` as a small public facade with explicit re-exports. Preserve existing public paths and signatures unless the task requests an API change.
3. Use child modules for related implementation that shares private state. Limit visibility to the parent or crate as needed; avoid widening the public API to make a move compile.
4. Import dependencies explicitly. Do not rely on a parent module's incidental private imports or introduce circular dependencies between modules.
5. Keep focused tests near their domain, or use a dedicated test module for cross-module behavior. Retain behavior assertions rather than replacing them with file-layout checks.
6. Separate refactoring from behavior changes. Preserve existing input, rendering, authentication, shutdown, and recovery behavior, including changes already present in the working tree.

## Current library boundaries

| Crate | Responsibilities |
| --- | --- |
| `agx-core` | `models.rs` owns snapshots and counters; `parsers.rs` owns pure counter parsing; `sampler.rs` owns Linux collection; `vnc.rs` observes VNC TCP sockets and GUI resident memory without connecting or controlling services. |
| `agx-net` | `protocol.rs` owns wire models; `client.rs` owns socket requests; `ssid.rs` owns byte encoding and scan parsing; `credentials.rs` owns validation and PSK derivation; `service.rs` owns the existing service implementation. |
| `agx-gui` (standalone) | `graphics/detection.rs` discovers connected HDMI outputs at startup and rejects kernel-forced connections without monitor EDID; `graphics.rs` resolves automatic or explicit renderer selection before threads start. `graphics/kms_input.rs` bridges physical Super keys and native scroll events missing from Slint’s LinuxKMS mapping into the current login/desktop window, tracking native pointer position for scroll targeting. `graphics/virtual_display.rs` owns the optional resizable VNC-only software platform and its event loop. `remote/` owns optional VNC access: `rfb.rs` is the pure wire protocol, `resize.rs` validates single-screen resize requests and encodes layout replies, `frame.rs` captured pixels and change detection, `input.rs` key/pointer translation into Slint events, `server.rs` shared state, listeners and client lifecycle, `server/session.rs` established sessions, framebuffer streaming and message dispatch, `server/tests.rs` live socket and request scheduling tests, `handoff.rs` passing the listener and viewers from the login screen to the desktop across the exec, `mod.rs` configuration, UI-thread capture and resize callbacks. `wm.rs` and `login.rs` retain their existing composition and authentication responsibilities. |
| `agx-tui` | `app/mod.rs` owns state/navigation; `app/render.rs` owns chrome/composition; `app/screens.rs` owns built-in read-only screens; `runtime.rs` owns the event loop; `terminal.rs` owns terminal cleanup/signals and suspend/resume; `shell.rs` owns foreground process groups, shell/tmux launch and restoring terminal ownership/modes; `app/shell.rs` renders the Terminal launch screen; `worker.rs` owns sampling; `palette.rs` owns shared styling; `remote_access.rs` composes the shared VNC status panel on Network and Services. Feature screens retain their own modules. |

The GUI launcher has its own boundaries: `src/launcher.rs` owns the app catalogue, search ranking, secondary commands and model updates; `ui/launcher.slint` owns the responsive overlay, keyboard navigation and scrolling; `ui/icons/apps/` contains its embedded vector icons. `main.rs` calls the launcher methods without owning their implementation.

Desktop gestures live in `src/input.rs`, including keyboard routing, drag lifecycle and title-bar double-click recognition. `wm.rs` retains the desktop model, workspace membership and scene composition; `wm/startup.rs` owns the initial panel composition and windowed layout defaults; `wm/tiling.rs` owns split-tree algorithms, `wm/scene.rs` spatial queries and hit testing, `wm/windowed.rs` floating geometry, collapse/restore and layout transitions, and `wm/tests.rs` the model behavior suite. `wm.rs` remains on the review backlog for its existing composition/model role; desktop state and startup wiring now have the named boundaries documented below.

Some feature/service files remain large and are candidates for later focused passes. This library refactor does not claim to have segmented the entire repository.

`scripts/trace-gui.py` owns workstation-side diagnostic collection, bounded log storage, SSH reconnection, and collector cleanup. It reads the Pi's existing logs and process state without changing GUI or system configuration.

`agx-init/src/device_events.rs` owns desktop device permissions and the kernel-to-libudev-zero input lifecycle relay. Its `bin/agx-display-devices.rs` entry point only starts that service. The existing supervised `agx-display-access` wrapper executes it when installed, retaining the permission-only fallback for older images. Device arrival notifications are forwarded after group access is ready; keystroke data never enters this relay.

## Validation

Run the affected crates' existing tests. For moves across these shared library boundaries, use `scripts/cargo.sh test --workspace --locked -- --test-threads=1` and `scripts/cargo.sh test --manifest-path crates/agx-gui/Cargo.toml --locked`. The GUI is a standalone workspace and is not covered by the root workspace command. Review the diff for altered signatures, visibility, initialization order, and discarded working-tree edits. A pure structural refactor should not require deploying new binaries to the Pi to change product behavior.

The embedded Web app lives in `apps/web/`: `host.c` owns the WPE lifecycle, settings and user data, `platform.c` owns offscreen display/view and bounded frame presentation, `gpu.c` sends bounded DMA-BUF descriptors, `protocol.c` owns the private control channel, `dialogs.c` owns form menus, script dialogs and new-window navigation policy, and `storage.c` owns private NVMe cache/data selection. In the GUI, `browser/protocol.rs` owns validation/encoding and key conversion, `browser/engine.rs` owns process isolation and off-thread frame/IPC transport, `browser/input.rs` maps desktop input, `browser/storage.rs` prepares protected per-user NVMe stores from the root greeter, and `browser/mod.rs` composes lifecycle, geometry and native chrome state. `ui/browser.slint` owns browser chrome and frame display; `ui/browser-popup.slint` owns native option and script popups. `app.rs` owns browser state and close-command coordination; `runtime.rs` installs its callbacks and timer. `wm.rs` only adds a window kind.

The native Web presentation bridge is isolated under `browser/gpu/`: `transport.rs` validates descriptor messages and owns received file descriptors; `egl.rs` imports DMA-BUF images and manages EGL fences; `renderer.rs` owns reusable GL textures and frame completion; `completion.rs` waits for exported native fences off the UI thread; `mod.rs` wires Slint rendering callbacks and CPU fallback diagnostics. Software/VNC presentation retains the shared-memory path. GPU resources are created and destroyed only with the Slint GL context current.

The GUI entry point now owns startup and signal handling. `app.rs` owns desktop state, command/session dispatch and layout synchronization; `runtime.rs` wires windows, feature callbacks, timers and sampling; `system_ui.rs` formats system snapshots and the optional preview palette; `pointer_tests.rs` retains the existing desktop interaction checks. `wm.rs` keeps its existing composition/model responsibilities; the terminal addition only extends its window-kind catalogue.

Native Terminal is isolated under `src/terminal/`: `session.rs` owns Alacritty's PTY/parser threads, shell startup, resize and child cleanup; `keys.rs` encodes xterm keys and bracketed paste; `render.rs` converts terminal cells to reusable styled row runs; `input.rs` handles selection, mouse reports, scrolling, terminal shortcuts and chrome hits; `mod.rs` composes sessions, tabs, search and model updates. `ui/terminal.slint` owns its native chrome and glyph presentation. `scripts/build-terminal.sh`, `package-terminal-runtime.py` and `deploy-terminal.sh` build and stage the bounded CLI runtime and locale data without replacing boot libraries.

OpenCode's vendor executable is pinned in `buildroot/package/agx-opencode/`; `apps/opencode/launch.sh` owns its private NVMe environment and preserves the caller's working directory. `scripts/build-opencode.sh`, `package-opencode-runtime.py` and `deploy-opencode.sh` handle its verified binary and Git/search runtime. The packager preserves Git's hard links and leaves OpenCode's embedded runtime unstripped. GUI launch uses a Terminal session directly; the TUI reuses its existing foreground-process handoff. The data-volume mount script prepares the sticky per-account storage parent after mounting.

The development swarm lives in `apps/agent/`: `agent.sh` owns private runtime
environment setup; `main.py` dispatches commands; `ui.py` owns the build-brief
form; `session.py` owns Herdr lifecycle and queued delivery; `messaging.py`
owns durable peer mail. Role profiles and the development skill are separate
assets. `angi-root.c` is the small administrator execution boundary;
`sdk-env.sh` and `angi-build.sh` own native build configuration. SDK preparation,
provisioning and runtime deployment are separate scripts under `scripts/`.

Desktop preferences live in `agx-gui/src/settings.rs`: validated per-account mouse settings, atomic storage, live libinput device configuration and hotplug reapplication. `ui/settings.slint` owns the responsive Settings tile content, mouse controls and test area; `ui/app.slint` hosts it in the regular window frame and exposes it through the session menu. `ui/desktop-input.slint` owns shared desktop pointer forwarding behind windows and above their read-only content, allowing native Settings controls to receive input within normal window stacking. `graphics/kms_input.rs` forwards device lifecycle events and applies the configured native scroll multiplier.

`agx-gui/src/session_refresh.rs` owns authenticated desktop restart and tracks asynchronous terminal/browser cleanup jobs so refresh can finish them before exec. Normal window closure remains asynchronous. `main.rs` invokes restart only after the desktop runtime has returned and released its UI state.

The GUI file-size heat map is isolated in `src/storage/`: `scan.rs` discovers mounted SD/NVMe filesystems through mountinfo and kernel device IDs, and bounds a personal-file walk without following symlinks or counting hard links twice; `policy.rs` limits cleanup to owned, removable files in personal storage and excludes system/configuration paths; `risk.rs` estimates and explains removal risk from location/type independently of size; `delete.rs` pins parent directories and verifies the confirmed file before removal; `worker.rs` runs scanning and deletion independently and publishes deletion completion before rescanning; `mod.rs` wires UI callbacks; `view.rs` owns persistent tile models, stable cleanup batches and explicit refresh epochs, preserving removed-path exclusions across manual refreshes for the desktop session, even if an app recreates them; `tests.rs` exercises mount discovery, file sizing and scan limits. `ui/storage.slint` owns the size-colored squares and file selection; `ui/storage-dialog.slint` owns details, keyboard navigation and the explicit delete confirmation. `ui/panels.slint` composes the fixed overview header, scrollbar-free drive-map viewport and fixed bottom-edge hover readout. The overview sits above the shared gesture layer so the maps receive hover events; title bars and borders retain desktop gestures.
