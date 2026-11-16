//! Workstation-staged update checks. Installation is intentionally unavailable.
use super::{Options, Palette};
use ratatui::crossterm::event::{
    Event as Input, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
};
use zeroize::{Zeroize, Zeroizing};
const PACKAGE: &str = "/var/lib/agx-update/candidate.tar.gz";
const DIRECTORY: &str = "/var/lib/agx-update";

pub(super) enum Event {
    Phase(&'static str, u64, u64),
    Done(String),
    Failed(String),
}
pub(super) struct Ui {
    receiver: Option<Receiver<Event>>,
    pub button: Rect,
    pub token_button: Rect,
    pub staged_button: Rect,
    pub shutdown_button: Rect,
    dialog_save: Rect,
    dialog_cancel: Rect,
    pub running: bool,
    phase: &'static str,
    done: u64,
    total: u64,
    message: String,
    failed: bool,
    token_dialog: bool,
    token: Zeroizing<String>,
    dialog_usable: bool,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            receiver: None,
            button: Rect::default(),
            token_button: Rect::default(),
            staged_button: Rect::default(),
            shutdown_button: Rect::default(),
            dialog_save: Rect::default(),
            dialog_cancel: Rect::default(),
            running: false,
            phase: "Idle",
            done: 0,
            total: 0,
            message:
                "Press C to check GitHub; G sets an optional token; V verifies a staged package."
                    .into(),
            failed: false,
            token_dialog: false,
            token: Zeroizing::new(String::new()),
            dialog_usable: false,
        }
    }
}
impl Ui {
    pub fn start_staged(&mut self) {
        if self.running {
            return;
        }
        let (send, receive) = mpsc::channel();
        match std::thread::Builder::new()
            .name("agx-update-check".into())
            .spawn(move || {
                if let Err(error) = check(PACKAGE, DIRECTORY, &send) {
                    let _ = send.send(Event::Failed(error.to_string()));
                }
            }) {
            Ok(_) => {
                self.receiver = Some(receive);
                self.running = true;
                self.phase = "Checking package";
                self.done = 0;
                self.total = 0;
                self.failed = false;
                self.message.clear();
            }
            Err(error) => {
                self.failed = true;
                self.message = format!("Could not start check: {error}");
            }
        }
    }
    fn service_request(&mut self, request: agx_net::Request, phase: &'static str) {
        if self.running {
            return;
        }
        let (send, receive) = mpsc::channel();
        match std::thread::Builder::new()
            .name("agx-github".into())
            .spawn(move || match agx_net::request(&request) {
                Ok(reply) if reply.ok => {
                    let _ = send.send(Event::Done(reply.message));
                }
                Ok(reply) => {
                    let _ = send.send(Event::Failed(reply.message));
                }
                Err(error) => {
                    let _ = send.send(Event::Failed(error.to_string()));
                }
            }) {
            Ok(_) => {
                self.receiver = Some(receive);
                self.running = true;
                self.phase = phase;
                self.done = 0;
                self.total = 0;
                self.failed = false;
                self.message.clear();
            }
            Err(error) => {
                self.failed = true;
                self.message = format!("Could not start request: {error}");
            }
        }
    }
    pub fn start_github(&mut self) {
        self.service_request(agx_net::Request::CheckGitHubRelease, "Checking GitHub");
    }
    pub fn token_dialog(&mut self) {
        if !self.running {
            self.token.zeroize();
            self.token_dialog = true;
        }
    }
    pub fn dialog_open(&self) -> bool {
        self.token_dialog
    }
    fn submit_token(&mut self) {
        if self.dialog_usable {
            let token = std::mem::replace(&mut self.token, Zeroizing::new(String::new()));
            self.token_dialog = false;
            self.service_request(agx_net::Request::SetGitHubToken { token }, "Saving token");
        }
    }
    pub fn dialog_input(&mut self, event: &Input) {
        match event {
            Input::Paste(value)
                if value.len() + self.token.len() <= 255
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') =>
            {
                self.token.push_str(value)
            }
            Input::Key(key) => match key.code {
                KeyCode::Esc => {
                    self.token.zeroize();
                    self.token_dialog = false;
                }
                KeyCode::Backspace => {
                    self.token.pop();
                }
                KeyCode::Enter if self.dialog_usable => self.submit_token(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && (c.is_ascii_alphanumeric() || c == '_' || c == '-')
                        && self.token.len() < 255 =>
                {
                    self.token.push(c)
                }
                _ => (),
            },
            Input::Mouse(mouse)
                if mouse.kind == MouseEventKind::Down(MouseButton::Left) && self.dialog_usable =>
            {
                if self.dialog_save.contains((mouse.column, mouse.row).into()) {
                    self.submit_token();
                } else if self
                    .dialog_cancel
                    .contains((mouse.column, mouse.row).into())
                {
                    self.token.zeroize();
                    self.token_dialog = false;
                }
            }
            _ => (),
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.receiver {
            while let Ok(event) = rx.try_recv() {
                changed = true;
                match event {
                    Event::Phase(phase, done, total) => {
                        self.phase = phase;
                        self.done = done;
                        self.total = total;
                    }
                    Event::Done(message) => {
                        self.running = false;
                        self.message = message;
                        self.failed = false;
                    }
                    Event::Failed(message) => {
                        self.running = false;
                        self.message = message;
                        self.failed = true;
                    }
                }
            }
        }
        if !self.running {
            self.receiver = None;
        }
        changed
    }
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        p: Palette,
        options: Options,
        kernel: &str,
    ) {
        let boot = fs::read_to_string("/run/agx-ssh-ready-seconds")
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .map(|s| format!("{s:.2} seconds"))
            .unwrap_or_else(|| "Available after next reboot".into());
        let outer = super::App::new(options).block(" System ", p.cyan);
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        let ratio = self
            .done
            .saturating_mul(20)
            .checked_div(self.total)
            .unwrap_or(0)
            .min(20) as usize;
        let bar = if self.running && self.total == 0 {
            let position = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as usize
                % 20;
            format!("[{}>{}]", "-".repeat(position), "-".repeat(19 - position))
        } else {
            format!("[{}{}]", "#".repeat(ratio), "-".repeat(20 - ratio))
        };
        let percent = self
            .done
            .saturating_mul(100)
            .checked_div(self.total)
            .map(|p| format!(" {p}%"))
            .unwrap_or_default();
        let lines = vec![
            Line::styled(
                "SYSTEM HEALTH & UPDATES",
                Style::default().fg(p.cyan).bold(),
            ),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Kernel              ", Style::default().fg(p.muted)),
                Span::styled(kernel.to_owned(), Style::default().fg(p.text).bold()),
            ]),
            Line::from(vec![
                Span::styled("SSH recovery ready  ", Style::default().fg(p.muted)),
                Span::styled(boot, Style::default().fg(p.green).bold()),
            ]),
            Line::styled(
                "Kernel start to SSH + USB ready.",
                Style::default().fg(p.muted),
            ),
            Line::raw(""),
            Line::styled(
                "UPDATES / KERNEL + SYSTEM",
                Style::default().fg(p.purple).bold(),
            ),
            Line::styled(
                if self.running {
                    " [ Checking... ] "
                } else {
                    " [ Check GitHub  C ] "
                },
                Style::default()
                    .fg(p.bg)
                    .bg(if self.running { p.muted } else { p.cyan })
                    .bold(),
            ),
            Line::styled(
                "G  Optional token |   V  Verify staged package",
                Style::default().fg(p.cyan),
            ),
            Line::styled(
                format!("{bar}{percent}  {}", self.phase),
                Style::default().fg(if self.failed { p.red } else { p.green }),
            ),
            Line::styled(
                self.message.clone(),
                Style::default().fg(if self.failed { p.amber } else { p.text }),
            ),
            Line::styled(
                "INSTALL  [--------------------]  Installer unavailable",
                Style::default().fg(p.muted),
            ),
            Line::styled(
                "Accounts, Wi-Fi and recovery must be preserved before installation is enabled.",
                Style::default().fg(p.muted),
            ),
            Line::raw(""),
            Line::raw("F10  Reboot with confirmation"),
        ];
        self.shutdown_button = if inner.height > 1 {
            Rect::new(inner.x, inner.bottom() - 1, inner.width.min(36), 1)
        } else {
            Rect::default()
        };
        self.button = if inner.height > 7 {
            Rect::new(inner.x, inner.y + 7, inner.width.min(28), 1)
        } else {
            Rect::default()
        };
        self.token_button = if inner.height > 8 {
            Rect::new(inner.x, inner.y + 8, inner.width.min(17), 1)
        } else {
            Rect::default()
        };
        self.staged_button = if inner.height > 8 && inner.width > 22 {
            Rect::new(
                inner.x + 22,
                inner.y + 8,
                inner.width.saturating_sub(22).min(23),
                1,
            )
        } else {
            Rect::default()
        };
        let body = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(1),
        );
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
        frame.render_widget(
            Paragraph::new("F9   Power off with confirmation").style(Style::default().fg(p.cyan)),
            self.shutdown_button,
        );
        if self.token_dialog {
            let screen = frame.area();
            self.dialog_usable = screen.width >= 60 && screen.height >= 16;
            let width = screen.width.min(76);
            let height = screen.height.min(13);
            let rect = Rect::new(
                (screen.width - width) / 2,
                (screen.height - height) / 2,
                width,
                height,
            );
            frame.render_widget(Clear, rect);
            let block = super::App::new(options).block(" GitHub access ", p.purple);
            let inside = block.inner(rect);
            frame.render_widget(block, rect);
            if !self.dialog_usable {
                frame.render_widget(
                    Paragraph::new("Resize to at least 60 x 16. Esc cancels.")
                        .wrap(Wrap { trim: false }),
                    inside,
                );
                return;
            }
            let text = vec![
                Line::styled("OPTIONAL GITHUB ACCESS", Style::default().fg(p.cyan).bold()),
                Line::raw(""),
                Line::raw("Optional: public release checks need no token."),
                Line::raw("Token is stored root-only and is never shown or logged."),
                Line::raw(""),
                Line::styled(
                    format!("TOKEN  {}", "*".repeat(self.token.chars().count().min(50))),
                    Style::default().fg(p.green).bold(),
                ),
                Line::raw(""),
                Line::raw(" [ Save ]    [ Cancel ]  |  Empty removes token"),
            ];
            self.dialog_save = Rect::new(inside.x, inside.y + 7, 10, 1);
            self.dialog_cancel = Rect::new(inside.x + 13, inside.y + 7, 10, 1);
            frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inside);
        }
    }
}
fn trusted(path: &str, directory: &str) -> io::Result<()> {
    for (name, dir) in [(directory, true), (path, false)] {
        let m = fs::symlink_metadata(name).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    e.kind(),
                    "No update package staged. Build and transfer one from the workstation.",
                )
            } else {
                e
            }
        })?;
        if m.file_type().is_symlink()
            || (dir && !m.is_dir())
            || (!dir && !m.is_file())
            || m.uid() != 0
            || m.mode() & 0o022 != 0
        {
            return Err(io::Error::other(
                "Update package and directory must be root-owned and not writable by others.",
            ));
        }
    }
    Ok(())
}
fn archive_reader(path: &str, mode: &str, entry: Option<&str>) -> io::Result<(Child, Child)> {
    let mut gzip = Command::new("/bin/gzip")
        .args(["-dc", path])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let input = gzip
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Cannot read compressed package"))?;
    let mut command = Command::new("/bin/tar");
    command
        .args([mode, "-"])
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(entry) = entry {
        command.arg(entry);
    }
    match command.spawn() {
        Ok(tar) => Ok((gzip, tar)),
        Err(error) => {
            let _ = gzip.kill();
            let _ = gzip.wait();
            Err(error)
        }
    }
}
fn tar_output(path: &str, mode: &str, entry: Option<&str>, limit: u64) -> io::Result<Vec<u8>> {
    let (mut gzip, mut tar) = archive_reader(path, mode, entry)?;
    let mut bytes = Vec::new();
    tar.stdout
        .take()
        .ok_or_else(|| io::Error::other("Cannot read archive"))?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    let status = tar.wait()?;
    let gzip_status = gzip.wait()?;
    if !status.success()
        || (entry.is_none() && !gzip_status.success())
        || bytes.len() as u64 > limit
    {
        return Err(io::Error::other("Unable to read update package."));
    }
    Ok(bytes)
}
fn check(path: &str, directory: &str, send: &Sender<Event>) -> io::Result<()> {
    trusted(path, directory)?;
    let listing = tar_output(path, "-tf", None, 65536)?;
    let mut names: Vec<_> = std::str::from_utf8(&listing)
        .map_err(|_| io::Error::other("Invalid archive listing"))?
        .lines()
        .collect();
    names.sort_unstable();
    if names != ["Image", "manifest.json", "rootfs.ext4"] {
        return Err(io::Error::other("Package contents are invalid."));
    }
    let manifest = tar_output(path, "-xOf", Some("manifest.json"), 16384)?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest)
        .map_err(|_| io::Error::other("Invalid update manifest."))?;
    if manifest["format"] != "agx-system-update-v1"
        || manifest["architecture"] != "aarch64"
        || manifest["board"] != "raspberry-pi-5"
    {
        return Err(io::Error::other("Update package is not for this Pi."));
    }
    let components = ["Image", "rootfs.ext4"];
    let mut total = 0u64;
    for name in components {
        let size = manifest["components"][name]["bytes"]
            .as_u64()
            .ok_or_else(|| io::Error::other("Missing component size"))?;
        if size == 0 || size > 1024 * 1024 * 1024 {
            return Err(io::Error::other("Component size is invalid."));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| io::Error::other("Invalid package size"))?;
    }
    let mut done = 0u64;
    for name in components {
        let expected_size = manifest["components"][name]["bytes"].as_u64().unwrap();
        let expected_hash = manifest["components"][name]["sha256"]
            .as_str()
            .ok_or_else(|| io::Error::other("Missing component hash"))?;
        let phase = if name == "Image" {
            "Verifying kernel"
        } else {
            "Verifying system"
        };
        let (mut gzip, mut child) = archive_reader(path, "-xOf", Some(name))?;
        let mut input = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Cannot read component"))?;
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buf = [0u8; 65536];
        let mut last = 0u64;
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > expected_size {
                let _ = child.kill();
                let _ = child.wait();
                let _ = gzip.kill();
                let _ = gzip.wait();
                return Err(io::Error::other("Component exceeds declared size."));
            }
            hash.update(&buf[..n]);
            if count - last >= 1024 * 1024 {
                last = count;
                let _ = send.send(Event::Phase(phase, done + count, total));
            }
        }
        let status = child.wait()?;
        let _ = gzip.wait();
        if !status.success()
            || count != expected_size
            || format!("{:x}", hash.finalize()) != expected_hash
        {
            return Err(io::Error::other(format!(
                "{phase} failed integrity verification."
            )));
        }
        done += count;
        let _ = send.send(Event::Phase(phase, done, total));
    }
    let _ = send.send(Event::Done(
        "Kernel and system package verified. Installation is not available yet.".into(),
    ));
    Ok(())
}
