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
| `agx-core` | `models.rs` owns snapshots and counters; `parsers.rs` owns pure counter parsing; `sampler.rs` owns Linux collection. |
| `agx-net` | `protocol.rs` owns wire models; `client.rs` owns socket requests; `ssid.rs` owns byte encoding and scan parsing; `credentials.rs` owns validation and PSK derivation; `service.rs` owns the existing service implementation. |
| `agx-gui` (standalone) | `remote/` owns optional VNC access: `rfb.rs` is the pure wire protocol, `frame.rs` captured pixels and change detection, `input.rs` key/pointer translation into Slint events, `server.rs` sockets and client threads, `handoff.rs` passing the listener and viewers from the login screen to the desktop across the exec, `mod.rs` configuration and the UI-thread capture timer. `main.rs`, `wm.rs` and `login.rs` predate this guide and remain on the size backlog. |
| `agx-tui` | `app/mod.rs` owns state/navigation; `app/render.rs` owns chrome/composition; `app/screens.rs` owns built-in read-only screens; `runtime.rs` owns the event loop; `terminal.rs` owns terminal cleanup/signals; `worker.rs` owns sampling; `palette.rs` owns shared styling. Feature screens retain their own modules. |

Some feature/service files remain large and are candidates for later focused passes. This library refactor does not claim to have segmented the entire repository.

## Validation

Run the affected crates' existing tests. For moves across these shared library boundaries, use `scripts/cargo.sh test --workspace --locked -- --test-threads=1` and `scripts/cargo.sh test --manifest-path crates/agx-gui/Cargo.toml --locked`. The GUI is a standalone workspace and is not covered by the root workspace command. Review the diff for altered signatures, visibility, initialization order, and discarded working-tree edits. A pure structural refactor should not require deploying new binaries to the Pi to change product behavior.
