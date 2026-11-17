# ANGI Terminal

Open **Terminal** from Apps (**Super+Space**), or press **Super+Enter**. It is a native, resizable ANGI window with the same tiled, windowed, fullscreen, workspace and header-collapse behavior as Web. System Console remains available separately for the existing TUI.

The terminal uses pinned **alacritty_terminal 0.26.0**, the terminal engine from Alacritty, with ANGI's own Slint interface. This embeds Alacritty's VT parser, grid, PTY event loop, synchronized updates, scrollback and job control. It does not run the standalone Alacritty window or inherit all of its frontend features. The existing Slint renderer draws cached glyphs on HDMI; the same interface works through the software/VNC backend. There is no browser, Electron, GTK or additional compositor in this app.

## Tools and features

The CLI runtime includes Bash 5.2.37 with Readline, tmux 3.6b, Vim 9.2.0943 with its runtime, less 704, ncurses terminal descriptions and the OpenSSH 10.6p1 client/key tools. These versions follow the repository's pinned Buildroot release. Only client tools are installed by the terminal deployment; the recovery SSH service remains as configured. `/bin/sh` remains the existing system shell. Terminal prefers Bash; `AGX_TERMINAL_SHELL` can select another absolute executable. Bash loads ANGI's defaults, then the user's `~/.bashrc`.

Terminal supports eight independent tabs, 10,000 scrollback lines per tab, regex search through scrollback, mouse selection, rectangular selection with Alt, 256 colors and truecolor, Unicode/wide text, bold/italic/underline/strikeout, alternate screens, application cursor keys, function keys, bracketed paste, focus reports, and xterm/SGR mouse reporting. Hold Shift to select or scroll history when an application owns the mouse. tmux provides persistent sessions and split panes; normal tabs end their shell when closed. A shell that exits leaves its final output visible until the tab is closed.

| Shortcut | Action |
| --- | --- |
| Super+Enter | Open/focus Terminal |
| Ctrl+Shift+T | New tab |
| Ctrl+Shift+W | Close current tab |
| Ctrl+PageUp / PageDown | Previous / next tab |
| Ctrl+Shift+C / V | Copy selection / paste |
| Ctrl+Shift+F | Toggle scrollback search; Enter searches forward |
| Escape | Close search |
| Shift+PageUp / PageDown | Scroll history |
| Ctrl+Shift+Plus / Minus, Ctrl+wheel | Change font size |
| Ctrl+C / Ctrl+Z | Interrupt / suspend the foreground shell job |

The tab bar also has new-tab, search and return-to-bottom buttons. Search accepts regular expressions; its arrows search backward or forward. Copy/paste uses the platform clipboard when available and a local terminal clipboard on the compositor-less Pi. Clipboard synchronization with a VNC viewer is not provided by this app. Automatic OSC52 clipboard reads/writes are disabled; pasted terminal escape/control sequences are removed. Kitty extended keyboard and graphics protocols, Sixel graphics, URL opening, and automatic reconnection of ordinary tabs are outside this implementation.

## Performance and lifecycle

Shells start only when Terminal opens. Alacritty parses PTY output off the UI thread, while a 16 ms presentation timer coalesces output and updates only changed styled row runs. Idle sessions do not rebuild the grid or redraw text. Hidden/collapsed windows continue draining output into bounded scrollback without drawing it. Viewports are bounded to 400 columns by 160 rows; font size is 10–26 px. No page-sized pixel buffers are involved.

PTY resize uses `TIOCSWINSZ` through Alacritty's event loop, so interactive applications receive real dimensions. Cleanup runs off the UI thread, uses a pidfd to target the original shell, and escalates a stuck shell after a short grace period. Closing the PTY hangs up ordinary foreground jobs. An explicitly detached tmux session continues running by design; use `tmux attach` to return to it.

The minimal image needs actual UTF-8 locale data for tmux and correct character widths. New images generate `C.UTF-8`. Incremental deployment generates that locale on NVMe with the external toolchain's own target `localedef` and source data; it does not replace the appliance's libc or loader.

## Build and validate

Run `scripts/build-terminal.sh`, `make gui-build`, then `scripts/deploy-terminal.sh` with the usual `AGX_SSH_HOST` and optional `AGX_SSH_HOST_KEY_ALIAS`. Deployment keeps runtime data on NVMe, backs up existing destinations, and switches the GUI executable atomically. `AGX_TERMINAL_RUNTIME_ONLY=1` installs tools without restarting the desktop.

GUI tests cover key/control encoding, paste filtering, styled and wide-character rows, alternate screens, scrollback, and a real PTY resize/output/exit. The GUI workspace must be tested separately from the root workspace. On the Pi, verify Bash, tmux, Vim editing, Ctrl+C, tabs, selection, searching, real resizing, and child cleanup through VNC before testing HDMI. Disconnect the VNC viewer when measuring native rendering or throughput so snapshot capture does not dominate the result.

Research: [Alacritty's terminal library](https://docs.rs/alacritty_terminal/0.26.0/alacritty_terminal/), [Alacritty features and scope](https://github.com/alacritty/alacritty#features), [tmux documentation](https://github.com/tmux/tmux/wiki), and [Ghostty embedding status](https://github.com/ghostty-org/ghostty#cross-platform-libghostty-for-embeddable-terminals). Alacritty's separable Rust engine fits ANGI's existing renderer; the standalone Wayland/X11 frontend is not required.

A Pi 5 native test at 1024×768, with the VNC viewer disconnected, wrote 250,000 short numbered lines (`seq 1 250000`) through the PTY in 0.444 seconds. The whole GUI used about 67 MiB RSS after that workload and accumulated one CPU tick over a three-second idle sample. Four close/reopen cycles returned to 25 GUI file descriptors with no remaining child processes. These are integration measurements for this fixture and display mode, not comparisons against other terminal applications or guarantees for arbitrary output.
