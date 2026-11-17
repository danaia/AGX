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
| `agx-gui` (standalone) | `graphics/detection.rs` discovers connected HDMI outputs at startup and rejects kernel-forced connections without monitor EDID; `graphics.rs` resolves automatic or explicit renderer selection before threads start. `graphics/kms_keys.rs` bridges physical Super-key press/release events missing from Slint’s LinuxKMS mapping into the current login/desktop window. `graphics/virtual_display.rs` owns the optional resizable VNC-only software platform and its event loop. `remote/` owns optional VNC access: `rfb.rs` is the pure wire protocol, `resize.rs` validates single-screen resize requests and encodes layout replies, `frame.rs` captured pixels and change detection, `input.rs` key/pointer translation into Slint events, `server.rs` shared state, listeners and client lifecycle, `server/session.rs` established sessions, framebuffer streaming and message dispatch, `server/tests.rs` live socket and request scheduling tests, `handoff.rs` passing the listener and viewers from the login screen to the desktop across the exec, `mod.rs` configuration, UI-thread capture and resize callbacks. `main.rs`, `wm.rs` and `login.rs` predate this guide and remain on the size backlog. |
| `agx-tui` | `app/mod.rs` owns state/navigation; `app/render.rs` owns chrome/composition; `app/screens.rs` owns built-in read-only screens; `runtime.rs` owns the event loop; `terminal.rs` owns terminal cleanup/signals; `worker.rs` owns sampling; `palette.rs` owns shared styling; `remote_access.rs` composes the shared VNC status panel on Network and Services. Feature screens retain their own modules. |

The GUI launcher has its own boundaries: `src/launcher.rs` owns the app catalogue, search ranking, secondary commands and model updates; `ui/launcher.slint` owns the responsive overlay, keyboard navigation and scrolling; `ui/icons/apps/` contains its embedded vector icons. `main.rs` calls the launcher methods without owning their implementation.

Desktop gestures live in `src/input.rs`, including keyboard routing, drag lifecycle and title-bar double-click recognition. `wm.rs` retains the desktop model, workspace membership and scene composition; `wm/tiling.rs` owns split-tree algorithms, `wm/scene.rs` spatial queries and hit testing, `wm/windowed.rs` floating geometry, collapse/restore and layout transitions, and `wm/tests.rs` the model behavior suite. The remaining `main.rs` and `wm.rs` review-sized files stay focused on their existing state/composition roles; unrelated startup and data collection remain a staged backlog.

Some feature/service files remain large and are candidates for later focused passes. This library refactor does not claim to have segmented the entire repository.

`scripts/trace-gui.py` owns workstation-side diagnostic collection, bounded log storage, SSH reconnection, and collector cleanup. It reads the Pi's existing logs and process state without changing GUI or system configuration.

`agx-init/src/device_events.rs` owns desktop device permissions and the kernel-to-libudev-zero input lifecycle relay. Its `bin/agx-display-devices.rs` entry point only starts that service. The existing supervised `agx-display-access` wrapper executes it when installed, retaining the permission-only fallback for older images. Device arrival notifications are forwarded after group access is ready; keystroke data never enters this relay.

## Validation

Run the affected crates' existing tests. For moves across these shared library boundaries, use `scripts/cargo.sh test --workspace --locked -- --test-threads=1` and `scripts/cargo.sh test --manifest-path crates/agx-gui/Cargo.toml --locked`. The GUI is a standalone workspace and is not covered by the root workspace command. Review the diff for altered signatures, visibility, initialization order, and discarded working-tree edits. A pure structural refactor should not require deploying new binaries to the Pi to change product behavior.

The embedded Web app lives in `apps/web/`: `host.c` owns the WPE lifecycle, settings and user data, `platform.c` owns offscreen display/view and bounded frame presentation, `protocol.c` owns the private control channel, `dialogs.c` owns form menus, script dialogs and new-window navigation policy, and `storage.c` owns private NVMe cache/data selection. In the GUI, `browser/protocol.rs` owns validation/encoding and key conversion, `browser/engine.rs` owns process isolation and off-thread frame/IPC transport, `browser/input.rs` maps desktop input, `browser/storage.rs` prepares protected per-user NVMe stores from the root greeter, and `browser/mod.rs` composes lifecycle, geometry and native chrome state. `ui/browser.slint` owns browser chrome and frame display; `ui/browser-popup.slint` owns native option and script popups. The additions to the existing review-sized `main.rs` add browser state and route its callbacks, close command and timer; `wm.rs` only adds a window kind. Their existing unrelated responsibilities are unchanged.
