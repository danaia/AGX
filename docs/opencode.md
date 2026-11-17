# OpenCode on ANGI

Open **OpenCode** from Apps (**Super+Space**) to start it in its own native Terminal tab. It uses the same resize, tiling, fullscreen and workspace controls as Terminal. From the TUI, choose **8 Terminal**, then press **o** or click **OpenCode**. Quitting OpenCode returns to the TUI. From any shell, run `opencode` or `opencode /path/to/project`.

Run **/connect** inside OpenCode to choose a provider and complete its sign-in or API-key setup. Installation does not supply an account, copy credentials from another application, enable automatic permission approval, or submit model prompts. OpenCode's provider and permission settings remain under the user's control. GUI launches start in the user's home; shell/TUI launches preserve the caller's working directory.

## Runtime

ANGI pins the official **OpenCode 1.19.6 Linux ARM64 glibc CLI**, verified against its immutable release asset SHA-256. The executable includes its own runtime; no separate Node, Bun or desktop browser is required. It runs on demand as the signed-in user. Git 2.54.0 and ripgrep 14.1.1 are built with the existing Buildroot toolchain for repository and search operations.

The executable is approximately 203 MiB, substantially larger than the native Terminal. Incremental deployment stores it, Git, ripgrep and their non-boot libraries on NVMe and switches public command paths atomically. Do **not strip** the OpenCode executable: its embedded application must remain intact. Automatic self-updates are disabled for the managed binary; update the pinned package and checksum through ANGI deployment instead. No public server or startup daemon is enabled by this integration.

The launcher places each user's data, configuration, caches, state and temporary files under:

```text
/var/lib/agx-data/agx-opencode-users/<uid>/
  config/opencode/
  data/opencode/
  cache/opencode/
  state/opencode/
  tmp/
```

The root-owned sticky parent allows new accounts to create their own store without a privileged agent service. Stores and newly created files are private to the owning account. The wrapper enters and validates the actual store before writing, refusing symlink redirection or an entry owned by another account. The mount service prepares the parent after NVMe mounts; the installer prepares it immediately. A missing NVMe volume produces a clear error instead of putting large caches on the SD root. Application-specific `OPENCODE_CONFIG` / `OPENCODE_CONFIG_DIR` overrides and project configuration continue to follow upstream behavior.

## Build, deploy and validate

`make deploy-opencode` builds the pinned package and native Git/search tools, rebuilds GUI/CLI launch entries, installs the OpenCode runtime, then updates the interfaces. Existing Terminal runtime prerequisites can be installed with `make deploy-terminal`. The usual `AGX_SSH_HOST` and optional `AGX_SSH_HOST_KEY_ALIAS` select the Pi.

The Buildroot package is `agx-opencode`. New full images reserve a 1 GiB root filesystem for the complete browser, terminal and coding runtime; incremental installation does not resize or reformat the running SD card. `BR2_STRIP_EXCLUDE_FILES` preserves `opencode` during full-image finalization. The executable's Linux ARM64 load alignment and actual startup were checked against the Pi's 16 KiB-page kernel.

Validation includes `--version`, normal full-screen startup, the provider picker without signing in, exit, GUI/TUI launch, resizing, and returning terminal ownership to the TUI. `python3 scripts/test-opencode-storage.py` tests private permissions, symlink refusal, argument/cwd preservation and the NVMe requirement. Run `scripts/test-tui-terminal-ssh.py` with `AGX_TEST_OPENCODE=1` to include OpenCode in the real SSH PTY suite; it does not submit model prompts.

Upstream: [pinned release](https://github.com/anomalyco/opencode/releases/tag/v1.19.6), [official installation documentation](https://opencode.ai/docs/), and [CLI introduction](https://opencode.ai/docs/cli/). Upstream's current documentation also covers OpenCode 2; this appliance package deliberately follows the pinned GitHub stable release rather than silently migrating major versions.
