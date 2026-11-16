# AGX repository instructions for Codex

Read [docs/code-structure.md](docs/code-structure.md) before extending or refactoring source files. Apply it to Rust, Slint, and supporting code throughout this repository.

- Evaluate a file's responsibilities before adding more code. At roughly 500 lines, review its boundaries; at roughly 750 lines, split clear responsibilities before extending it, or explain why keeping it cohesive is better.
- Keep `lib.rs` and `main.rs` focused on exports, configuration, and startup. Aim for fewer than 150 lines in library entry points; put implementation in named modules.
- Extract by responsibility, such as models, parsing, transport, state, rendering, input, or lifecycle. Do not create arbitrary numbered parts, catch-all helpers, or trivial forwarding modules just to meet a line count.
- Preserve public APIs and runtime behavior during structural refactors. Keep imports explicit and visibility as narrow as practical. Preserve unrelated working-tree changes.
- Use existing behavior tests to validate a move. Run the relevant workspace tests and standalone GUI checks when shared APIs change. Do not add tests that merely check module names or file lengths.
- Run `python3 scripts/check-file-sizes.py` to identify files needing review. Existing large files are a staged backlog, not a reason to refactor unrelated subsystems during a focused change.

These are repository-scoped instructions. Keep the shared guide current when module responsibilities change.

## Product name: ANGI (technical name: AGX)

The public product name is **ANGI** (pronounced "Angie"). Anything a person sees must say ANGI: README and public docs prose, the TUI and GUI text, window titles, login and shutdown messages, `/etc/issue`, the login splash, and the ASCII logo in `brand/logo-ascii.md` (white "ANG", lime "I").

Keep **AGX** for technical identifiers: the `agx` command and its help text, `agx-*` crates and binaries, service and group names, file paths and environment variables (`/usr/share/agx`, `AGX_*`), Buildroot package names, (the GitHub repository itself is now `danaia/ANGI`). Do not rename identifiers when changing visible text, and do not change visible text to an identifier. When adding a user-visible string, write ANGI.

