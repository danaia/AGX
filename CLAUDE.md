# AGX repository instructions for Claude

Read and follow [AGENTS.md](AGENTS.md) and [docs/code-structure.md](docs/code-structure.md) before changing code in this repository. They are the shared source of truth for both Claude and Codex.

In particular, evaluate source files around 500 lines, split clear responsibilities around 750 lines before extending them, and keep `lib.rs`/`main.rs` focused on their entry-point roles. Prefer cohesive named modules over mechanical file splitting. Preserve public APIs, runtime behavior, and unrelated working-tree changes; validate with the existing tests.

Run `python3 scripts/check-file-sizes.py` to review the current large-file backlog. Explain any decision to retain an oversized file rather than silently growing it. Apply the shared guide's exceptions and validation requirements.
