//! Profile editor. Secrets are masked, bounded, and cleared on cancellation/submission.
use super::{Options, Palette};
use agx_net::{Reply, Request};
use agx_profile::{Save, Status, quality, validate};
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
};
use std::{
    io,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

pub(super) struct Worker {
    pub results: Receiver<(bool, Result<Reply, String>)>,
    commands: SyncSender<Request>,
}
impl Worker {
    pub fn start() -> io::Result<Self> {
        let (sender, results) = mpsc::sync_channel(4);
        let (commands, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("agx-profile".into())
            .spawn(move || {
                let mut next = Request::ProfileStatus;
                loop {
                    let saving = matches!(next, Request::SaveProfile(_));
                    let result = agx_net::request(&next).map_err(|e| e.to_string());
                    // Drop the request (and its zeroized password) before waiting for more input.
                    drop(next);
                    if sender.send((saving, result)).is_err() {
                        break;
                    }
                    next = match receiver.recv_timeout(Duration::from_secs(3)) {
                        Ok(value) => value,
                        Err(mpsc::RecvTimeoutError::Timeout) => Request::ProfileStatus,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                }
            })?;
        Ok(Self { results, commands })
    }
    pub fn send(&self, request: Request, ui: &mut Ui) {
        if self.commands.try_send(request).is_err() {
            ui.message = "Another profile operation is pending. Retry shortly.".into();
            ui.pending = false;
        }
    }
}
fn border(options: Options) -> Block<'static> {
    if options.ascii {
        Block::default().border_set(ratatui::symbols::border::Set {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "|",
            vertical_right: "|",
            horizontal_top: "-",
            horizontal_bottom: "-",
        })
    } else {
        Block::default().border_type(ratatui::widgets::BorderType::Rounded)
    }
}
#[derive(Default)]
pub(super) struct Ui {
    status: Option<Status>,
    selected: usize,
    name: String,
    hostname: String,
    username: String,
    password: Zeroizing<String>,
    confirm_password: Zeroizing<String>,
    editing: bool,
    create: bool,
    focus: usize,
    confirmation: bool,
    confirm_apply: bool,
    pub pending: bool,
    message: String,
    error: bool,
    fields: Vec<Rect>,
    actions: Vec<Rect>,
    account_rows: Vec<Rect>,
    confirm_buttons: Vec<Rect>,
    usable: bool,
}
impl Ui {
    pub fn update(&mut self, (saving, result): (bool, Result<Reply, String>)) {
        match result {
            Ok(reply) => {
                if let Some(status) = reply.profile {
                    self.status = Some(status);
                    if !self.editing && !self.confirmation {
                        self.load();
                    }
                }
                if saving {
                    self.pending = false;
                    self.error = !reply.ok;
                    self.message = reply.message;
                    if reply.ok {
                        self.editing = false;
                        self.load();
                    }
                }
            }
            Err(error) => {
                if saving || self.status.is_none() {
                    self.error = true;
                    self.message = error;
                }
                if saving {
                    self.pending = false;
                }
            }
        }
    }
    fn load(&mut self) {
        if let Some(status) = &self.status {
            self.hostname = status.hostname.clone();
            self.selected = self.selected.min(status.accounts.len().saturating_sub(1));
            if let Some(account) = status.accounts.get(self.selected) {
                self.username = account.username.clone();
                self.name = account.name.clone();
                self.create = false;
            } else {
                self.username.clear();
                self.name.clear();
                self.create = true;
            }
        }
        self.clear_secrets();
    }
    fn clear_secrets(&mut self) {
        self.password.zeroize();
        self.confirm_password.zeroize();
    }
    fn edit(&mut self, new: bool) {
        if self.pending || self.status.is_none() {
            return;
        }
        self.editing = true;
        self.focus = 0;
        self.confirmation = false;
        self.error = false;
        self.message.clear();
        self.clear_secrets();
        if new {
            self.name.clear();
            self.username.clear();
            self.create = true;
        }
    }
    fn draft(&self) -> Save {
        Save {
            name: self.name.clone(),
            hostname: self.hostname.clone(),
            username: self.username.clone(),
            password: Zeroizing::new(self.password.to_string()),
            create: self.create,
        }
    }
    fn review(&mut self) {
        if !self.usable {
            return;
        }
        if self.password.as_str() != self.confirm_password.as_str() {
            self.error = true;
            self.message = "Passwords do not match.".into();
            return;
        }
        match validate(&self.draft()) {
            Ok(()) => {
                self.confirmation = true;
                self.confirm_apply = false;
                self.error = false;
                self.message.clear();
            }
            Err(error) => {
                self.error = true;
                self.message = error;
            }
        }
    }
    fn text(&mut self, value: &str) {
        if value.chars().any(char::is_control) {
            return;
        }
        let field: &mut String = match self.focus {
            0 => &mut self.name,
            1 => &mut self.hostname,
            2 if self.create => &mut self.username,
            3 => &mut self.password,
            4 => &mut self.confirm_password,
            _ => return,
        };
        let limit = match self.focus {
            0 => 80,
            1 => 63,
            2 => 32,
            _ => 512,
        };
        if field.len() + value.len() <= limit {
            field.push_str(value);
            self.error = false;
            self.message.clear();
        }
    }
    pub fn event(&mut self, input: &Event) -> (bool, Option<Request>) {
        if self.confirmation {
            match input {
                Event::Key(key) => match key.code {
                    KeyCode::Esc => self.confirmation = false,
                    KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                        self.confirm_apply = !self.confirm_apply
                    }
                    KeyCode::Enter if self.confirm_apply && self.usable => {
                        let save = self.draft();
                        self.clear_secrets();
                        self.confirmation = false;
                        self.pending = true;
                        self.message = "Saving profile…".into();
                        return (true, Some(Request::SaveProfile(save)));
                    }
                    KeyCode::Enter => self.confirmation = false,
                    _ => (),
                },
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(index) = self
                        .confirm_buttons
                        .iter()
                        .position(|r| r.contains((mouse.column, mouse.row).into()))
                    {
                        if index == 0 {
                            self.confirmation = false;
                        } else if self.usable {
                            let save = self.draft();
                            self.clear_secrets();
                            self.confirmation = false;
                            self.pending = true;
                            self.message = "Saving profile…".into();
                            return (true, Some(Request::SaveProfile(save)));
                        }
                    }
                }
                _ => (),
            }
            return (true, None);
        }
        if let Event::Mouse(mouse) = input
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
        {
            if let Some(index) = self
                .fields
                .iter()
                .position(|r| r.contains((mouse.column, mouse.row).into()))
            {
                if self.editing {
                    self.focus = index;
                } else {
                    self.edit(false);
                    self.focus = index;
                }
                return (true, None);
            }
            if let Some(index) = self
                .actions
                .iter()
                .position(|r| r.contains((mouse.column, mouse.row).into()))
            {
                match index {
                    0 if self.editing => self.review(),
                    0 => self.edit(false),
                    1 => {
                        self.load();
                        self.edit(true);
                    }
                    _ => (),
                }
                return (true, None);
            }
            if !self.editing
                && let Some(index) = self
                    .account_rows
                    .iter()
                    .position(|r| r.contains((mouse.column, mouse.row).into()))
            {
                self.selected = index;
                self.load();
                return (true, None);
            }
        }
        if !self.editing {
            if let Event::Key(key) = input {
                match key.code {
                    KeyCode::Enter | KeyCode::Char('e') => {
                        self.edit(false);
                        return (true, None);
                    }
                    KeyCode::Char('n') => {
                        self.edit(true);
                        return (true, None);
                    }
                    KeyCode::Down => {
                        self.selected += 1;
                        self.load();
                        return (true, None);
                    }
                    KeyCode::Up => {
                        self.selected = self.selected.saturating_sub(1);
                        self.load();
                        return (true, None);
                    }
                    _ => (),
                }
            }
            return (false, None);
        }
        match input {
            Event::Paste(value) => self.text(value),
            Event::Key(key) => match key.code {
                KeyCode::Esc => {
                    self.editing = false;
                    self.load();
                    self.message = "Changes discarded.".into();
                    self.error = false;
                }
                KeyCode::Tab | KeyCode::Down => self.focus = (self.focus + 1) % 6,
                KeyCode::BackTab | KeyCode::Up => self.focus = (self.focus + 5) % 6,
                KeyCode::Enter if self.focus == 5 => self.review(),
                KeyCode::Enter => self.focus = (self.focus + 1) % 6,
                KeyCode::Backspace => {
                    let field: &mut String = match self.focus {
                        0 => &mut self.name,
                        1 => &mut self.hostname,
                        2 if self.create => &mut self.username,
                        3 => &mut self.password,
                        4 => &mut self.confirm_password,
                        _ => return (true, None),
                    };
                    field.pop();
                    self.error = false;
                    self.message.clear();
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.review()
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.text(&c.to_string())
                }
                _ => (),
            },
            _ => (),
        }
        (true, None)
    }
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, p: Palette, options: Options) {
        self.fields.clear();
        self.actions.clear();
        self.account_rows.clear();
        self.confirm_buttons.clear();
        self.usable = area.width >= 60 && area.height >= 16;
        let title = if options.ascii {
            " PROFILE / IDENTITY "
        } else {
            " ◉ PROFILE / IDENTITY "
        };
        let outer = border(options)
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(p.cyan));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        if !self.usable {
            frame.render_widget(Paragraph::new("Your account and device identity\n\nResize to at least 60 x 24 to edit profiles.\nEsc cancels editing; passwords stay masked.").wrap(Wrap{trim:false}),inner);
            return;
        }
        let panes = Layout::horizontal([
            Constraint::Min(35),
            Constraint::Length(if area.width >= 100 { 34 } else { 21 }),
        ])
        .spacing(2)
        .split(inner);
        let compact = inner.height < 22;
        let field_height = if compact { 2 } else { 3 };
        let rows = Layout::vertical([
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Length(field_height),
            Constraint::Length(field_height),
            Constraint::Length(field_height),
            Constraint::Length(field_height),
            Constraint::Length(field_height),
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Min(1),
        ])
        .split(panes[0]);
        frame.render_widget(
            Paragraph::new(if self.pending {
                "SAVING PROFILE…"
            } else if self.create {
                "MAKE THIS PI YOURS"
            } else {
                "YOUR ACCOUNT & DEVICE"
            })
            .style(Style::default().fg(p.purple).bold()),
            rows[0],
        );
        let labels = [
            "Full name",
            "Hostname · replaces agx",
            "Username · SSH login",
            "Password",
            "Confirm password",
        ];
        let values = [
            self.name.clone(),
            self.hostname.clone(),
            self.username.clone(),
            "*".repeat(self.password.chars().count()),
            "*".repeat(self.confirm_password.chars().count()),
        ];
        for i in 0..5 {
            let active = self.editing && self.focus == i;
            let value = if values[i].is_empty() {
                if i >= 3 && !self.create {
                    "Leave blank to keep password".into()
                } else {
                    "…".into()
                }
            } else {
                values[i].clone()
            };
            let value = if options.ascii {
                value.replace('…', "...").replace('·', "/")
            } else {
                value
            };
            let label = if options.ascii {
                labels[i].replace('·', "/")
            } else {
                labels[i].into()
            };
            let block = border(options)
                .borders(if compact { Borders::TOP } else { Borders::ALL })
                .title(format!(" {label} "))
                .border_style(Style::default().fg(if active { p.cyan } else { p.muted }));
            let inside = block.inner(rows[i + 1]);
            let visible: String = value
                .chars()
                .rev()
                .take(inside.width.saturating_sub(1) as usize)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            frame.render_widget(
                Paragraph::new(visible.clone())
                    .block(block)
                    .style(Style::default().fg(if active { p.text } else { p.muted })),
                rows[i + 1],
            );
            if active && inside.width > 0 {
                frame.set_cursor_position((
                    inside.x + (visible.chars().count() as u16).min(inside.width - 1),
                    inside.y,
                ));
            }
            self.fields.push(rows[i + 1]);
        }
        let q = quality(&self.password, &self.username, &self.name);
        let label = if self.password.is_empty() {
            "Quality meter is advisory / simple passwords allowed".into()
        } else {
            format!("{} / estimated password quality", q.label)
        };
        if options.ascii {
            frame.render_widget(
                Paragraph::new(format!(
                    "[{}{}] {label}",
                    "#".repeat((q.score / 25) as usize),
                    "-".repeat(4 - (q.score / 25) as usize)
                ))
                .style(Style::default().fg(p.amber)),
                rows[6],
            );
        } else {
            frame.render_widget(
                Gauge::default()
                    .ratio(q.score as f64 / 100.0)
                    .label(label)
                    .gauge_style(
                        Style::default()
                            .fg(if q.score >= 75 {
                                p.green
                            } else if q.score >= 50 {
                                p.amber
                            } else {
                                p.red
                            })
                            .bg(p.bg),
                    ),
                rows[6],
            );
        }
        let buttons = Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(rows[7]);
        for (i, label) in [
            if self.editing {
                " Review & save  Ctrl-S "
            } else {
                if self.create {
                    " Create profile  Enter "
                } else {
                    " Edit profile  Enter "
                }
            },
            " New user  N ",
        ]
        .iter()
        .enumerate()
        {
            frame.render_widget(
                Paragraph::new(*label).style(
                    Style::default()
                        .fg(p.bg)
                        .bg(if i == 0 { p.cyan } else { p.purple })
                        .bold(),
                ),
                buttons[i],
            );
            self.actions.push(buttons[i]);
        }
        let message = if self.message.is_empty() {
            if self.editing {
                "Tab next field / Ctrl-S review / Esc discard"
            } else {
                "Create an administrator to manage this Pi from the TUI."
            }
        } else {
            &self.message
        };
        frame.render_widget(
            Paragraph::new(message.replace('…', "..."))
                .style(Style::default().fg(if self.error { p.red } else { p.green }))
                .wrap(Wrap { trim: false }),
            rows[8],
        );
        let mut lines = vec![
            Line::styled("AGX ADMINISTRATORS", Style::default().fg(p.purple).bold()),
            Line::raw(""),
        ];
        if let Some(status) = &self.status {
            for (i, account) in status.accounts.iter().enumerate() {
                let row = panes[1].y + 2 + i as u16;
                if row < panes[1].bottom() {
                    self.account_rows
                        .push(Rect::new(panes[1].x, row, panes[1].width, 1));
                }
                lines.push(Line::styled(
                    format!(
                        "{} {}",
                        if i == self.selected { ">" } else { " " },
                        account.username
                    ),
                    Style::default().fg(if i == self.selected { p.cyan } else { p.text }),
                ));
            }
        }
        lines.extend([
            Line::raw(""),
            Line::raw("Password login"),
            Line::styled("SSH + console", Style::default().fg(p.green)),
            Line::raw(""),
            Line::raw("Connect as:"),
            Line::styled(
                format!(
                    "ssh {}@<Pi-IP>",
                    if self.username.is_empty() {
                        "username"
                    } else {
                        &self.username
                    }
                ),
                Style::default().fg(p.cyan),
            ),
            Line::raw(""),
            Line::raw("Root SSH key recovery"),
            Line::raw("remains available."),
            Line::raw(""),
            Line::raw("This account can manage"),
            Line::raw("users, Wi-Fi and reboot."),
            Line::raw(""),
            Line::raw("Tab / mouse navigation"),
            Line::raw("Esc discards edits"),
        ]);
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), panes[1]);
        if self.confirmation {
            self.draw_confirm(frame, p, options);
        }
    }
    fn draw_confirm(&mut self, frame: &mut Frame, p: Palette, options: Options) {
        let area = frame.area();
        let width = area.width.min(70);
        let height = 16.min(area.height);
        let rect = Rect::new(
            (area.width - width) / 2,
            (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, rect);
        let block = border(options)
            .borders(Borders::ALL)
            .title(" Apply profile ")
            .style(Style::default().bg(p.panel).fg(p.text))
            .border_style(Style::default().fg(p.purple));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
        let text = format!(
            "{} administrator account\n\nName: {}\nUsername: {}\nHostname: {}\n\nSSH and console will accept this account's password.\nHostname applies now and after reboot.\nRoot SSH key recovery is retained.\nThe development passwordless root console is locked.",
            if self.create { "Create" } else { "Update" },
            self.name,
            self.username,
            self.hostname
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), rows[0]);
        let buttons = Layout::horizontal([Constraint::Percentage(50); 2]).split(rows[1]);
        for (i, label) in [" Cancel ", " Apply profile "].iter().enumerate() {
            frame.render_widget(
                Paragraph::new(*label).style(
                    Style::default()
                        .fg(p.bg)
                        .bg(if self.confirm_apply == (i == 1) {
                            p.cyan
                        } else {
                            p.muted
                        })
                        .bold(),
                ),
                buttons[i],
            );
        }
        self.confirm_buttons = buttons.to_vec();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn profile_masks_password_and_cancel_clears_both_secrets() {
        let mut ui = Ui {
            status: Some(Status {
                hostname: "agx".into(),
                accounts: vec![],
            }),
            ..Ui::default()
        };
        ui.load();
        ui.edit(true);
        ui.password = Zeroizing::new("never-render-this-secret".into());
        ui.confirm_password = Zeroizing::new("never-render-this-secret".into());
        let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
        t.draw(|f| {
            ui.draw(
                f,
                f.area(),
                Palette::new(super::super::ColorMode::Never),
                Options::default(),
            )
        })
        .unwrap();
        let text: String = t
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!text.contains("never-render"));
        assert!(text.contains("********"));
        ui.error = true;
        ui.message = "Passwords do not match.".into();
        ui.focus = 4;
        ui.event(&Event::Key(KeyCode::Backspace.into()));
        assert!(!ui.error);
        assert!(ui.message.is_empty());
        ui.event(&Event::Key(KeyCode::Esc.into()));
        assert!(ui.password.is_empty());
        assert!(ui.confirm_password.is_empty());
    }
}
