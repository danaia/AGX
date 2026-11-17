# ANGI agent workshop

Run **`agent angi`** from a normal ANGI Terminal or the TUI's shell. Choose
**N**, describe what to build, then press Enter. The workshop opens Herdr with
three OpenCode agents: orchestrator on the left, builder and tester on the
right. Use **Ctrl+B, then Q** to detach while they continue working. Run
`agent angi` and choose **R** to resume. A new build creates a separate named
session; old sessions remain available in Herdr rather than being killed.

Choose **C** to open OpenCode, then run `/connect` to authenticate a model
provider. The agents use OpenCode's configured/default model and the current
account's provider credentials. Credentials are not copied from Codex or from
another account. Three agents can generate concurrent model usage.
The built-in Big Pickle model was also verified on the development Pi without
saved credentials; availability of that service is controlled by OpenCode.

The orchestrator receives the build brief and coordinates work. Builder edits
the shared checkout; tester independently validates changes. Their role
profiles tell them how to assign work and report results without spawning
additional agents. `agent tell builder 'message'` queues durable peer mail;
use orchestrator or tester for the other recipients. A small background
dispatcher submits queued messages when the receiving agent is idle/done.
Delivery means submission to OpenCode; model/provider failures still need
attention in that pane. `agent inbox` reads messages inside a swarm pane;
`agent status` shows the last swarm's live agents.

## Native development and administration

Source is `/var/lib/agx-data/angi-dev`, initialized as a local Git repository
from the workstation's current source, including uncommitted files. It is an
independent checkout; future edits are not automatically synchronized with the
workstation or pushed to GitHub. The SDK and caches are also on NVMe.

- `angi-build cli`: release CLI build on the Pi.
- `angi-build gui`: release LinuxKMS GUI build on the Pi.
- `angi-build test`: root workspace tests.
- `angi-build gui-test`: standalone GUI tests with LinuxKMS enabled.
- `source "$(angi-build env)"`: native build environment for custom commands.

The SDK combines checksum-pinned Debian ARM64 GCC 12/binutils, make, pkgconf
and Python 3.11 packages with the development image's own Buildroot sysroot,
plus Rust 1.97.1. Debian packages are extracted privately; no package manager
or Debian libc is installed into the live system. Compiler and Python wrappers
isolate their dependency libraries. Rust formatting and Clippy components are
included by the provisioning script. SDK package versions, hashes and upstream
copyright files are retained. Full Buildroot image/kernel rebuilds still need
a native-host toolchain configuration; the repository's image configuration
currently targets the workstation's cross toolchain.

The installed `angi-development` skill documents ANGI's source boundaries,
native builds, services, app installation and rollback. It is loaded from the
account's NVMe OpenCode configuration. Profiles also explicitly instruct each
agent to read it before work.

`angi-root COMMAND ARG...` grants full root execution to `agx-admin` members,
as requested for this workshop. The helper is a root-owned `4750` executable
on the system partition because NVMe is mounted `nosuid`. It validates group
membership, clears the inherited environment, and logs caller UID and program
name, excluding arguments. OpenCode and ordinary builds run as the user.
For shell redirection, use `angi-root /bin/sh -c '...'` explicitly.

## Provisioning

Install OpenCode and its terminal runtime first. With the matching Buildroot
sysroot present under `output/staging`, run:

```sh
AGX_SSH_HOST=root@10.78.0.2 AGX_SSH_HOST_KEY_ALIAS=10.77.0.2 make agent-sdk
AGX_SSH_HOST=root@10.78.0.2 AGX_SSH_HOST_KEY_ALIAS=10.77.0.2 make deploy-agent
```

`AGX_DEV_USER` defaults to `dana`. Provisioning preserves an existing source
workspace. The runtime is Herdr 0.9.5, verified against its published ARM64
release digest; deployments retain versioned releases on NVMe. The image
package `agx-agent` includes the launcher, Python modules, Herdr and root
helper; the SDK and source are separately provisioned development data.

OpenCode/Herdr state, plugin caches, delivery logs, prompts and mail are private
to each account under `/var/lib/agx-data/agx-opencode-users/<uid>`. Herdr's
official integration reports agent state and conversation IDs; its installer
is given a temporary private HOME because this version assumes a fixed
OpenCode configuration path. Actual agent HOME remains the normal user home.

Run `make agent-test` for model-free messaging/isolation tests. Native builds,
privilege checks, Herdr layout, attach/detach, and live model responses require
the Pi; test those separately and record provider failures accurately.

Verified on the development Pi on November 17, 2026: native workspace tests,
all 105 standalone GUI tests, a release CLI build, C/C++ compilation, allowed
and denied administrator execution, and five messaging/isolation tests. The
three live Big Pickle agents exchanged orchestrator-to-peer messages and a
builder/tester round trip. Detach/resume preserved the agents; stopping and
restarting their Herdr server restored all three native conversation IDs.
These checks did not rebuild a complete OS image or require an HDMI monitor.

Upstream: [Herdr 0.9.5](https://github.com/herdrdev/herdr/releases/tag/v0.9.5)
and the [Herdr CLI reference](https://herdr.dev/tui/docs/cli-reference/).
