# Profiles and login

AGX 0.4.0 opens the Rust TUI on **1 Profile**, followed by Overview, Hardware, Network, and Help. Profiles create real Linux administrator accounts with private home directories and persistent identity settings.

## Create your profile

Open the current USB recovery shell and run:

```sh
agx tui --color truecolor
```

Press Enter or click **Create profile**. Enter your full name, hostname, username, password, and password confirmation. Tab moves through fields; mouse clicks focus fields. Passwords are masked and support paste. Press Ctrl-S or choose **Review & save**, review the account and hostname, then select **Apply profile**. Confirmation initially selects Cancel.

Simple passwords are allowed. The quality meter is an estimate and never blocks a password for being weak. A new account requires a matching, nonempty password of 1–128 characters without control characters. No password is sent through shell arguments or printed in status messages.

The hostname replaces `agx` immediately and persists in `/etc/hostname`; the local hosts entry is updated too. Use lowercase letters, digits, and internal hyphens. Hostname changes do not automatically configure LAN DNS or mDNS, so connecting by the IP shown in Network remains reliable.

Usernames start with a lowercase letter and contain up to 32 lowercase letters, digits, underscores, or hyphens. Existing system accounts cannot be overwritten. Use **New user** (`n`) to create additional administrators. Select an existing account to change its name or password; leave both password fields blank to keep its current password. Usernames are fixed after creation.

The form supports 80×24 terminals and larger. Smaller terminals retain navigation and cancellation but block applying an incomplete view. Esc discards edits and clears both secret fields. Ctrl-C exits the TUI.

## Sign in

Choose the account in your SSH command:

```sh
ssh your_username@10.77.0.2
```

SSH prompts for that account's password. For Wi-Fi, replace the address with the current Wi-Fi IP. The existing device host key is retained. A local console prompts for the username and password after boot; AGX does not automatically sign in to a named account.

The artwork in [brand/logo-ascii.md](../brand/logo-ascii.md) appears at the console login prompt and as a colored welcome splash after interactive SSH or console authentication. The splash respects `NO_COLOR` and plain terminals. Noninteractive SSH commands, JSON output, and file transfers remain free of splash output.

Once the first account is saved, the development console's empty root password is locked. Root SSH remains available through its existing recovery key; root password authentication is disabled. Creating a profile does not reboot the Pi or close the current SSH session. `F10` continues to open the reboot confirmation.

## Implementation and deployment

`agx-profile` supplies shared validation, advisory quality estimates, and account operations. The existing root service, `agx-netd`, handles requests over the shared Unix socket. The socket is accessible to root and the `agx-admin` group; new accounts use this group to operate the TUI's profile, networking, and reboot controls without a setuid TUI or passwordless shell elevation.

Passwords are stored as salted SHA-512 crypt hashes with 100,000 rounds in root-only `/etc/shadow`. Account writes use the system password-database lock and synced replacement files. New accounts receive a mode `0700` home directory. Wi-Fi credentials remain in root-only configuration files; group access is limited to the service socket, not those files.

Build with `make build` and validate with `make check-image`. `make deploy-profile` builds and installs the profile/TUI update on the existing development Pi over root-key USB SSH. It updates login artwork and SSH policy and restarts the shared daemon, briefly interrupting Wi-Fi. It preserves accounts, passwords, saved networks, host keys, and the configured hostname.

## Verification — November 15, 2026

Physical Pi tests used a temporary account and a generated simple password, then removed the account and restored the original hostname:

- TUI account creation, live hostname change, masked paste, advisory quality meter, mismatch validation, review cancellation, 80×24 layout, and small-terminal submission guard.
- Successful password-based SSH and console login, incorrect-password rejection, and the ASCII welcome splash.
- Provisioning service access from the new administrator and continued root SSH key recovery.
- Existing dashboard keyboard/mouse, resize, terminal restoration, disconnect, and Wi-Fi provisioning regression checks.
- Rust tests, Clippy with warnings denied, ARM64 cross-compilation, complete Buildroot image build, and generated image checks.

Evidence lives in the ignored `.cache/evidence/profile-pty.json`, terminal captures beginning `profile-`, and `profile-image-validation.txt`. Passwords are generated in memory and excluded from reports. The installed system was tested live; a fresh flash/cold boot of the regenerated image remains a release check.
