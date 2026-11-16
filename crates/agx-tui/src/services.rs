//! Services and a bounded live lifecycle journal; all mutations go through the Rust supervisor.
use super::{Options, Palette};
use agx_init::{Action, Reply, Request};
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};
use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};
pub struct Update {
    reply: Result<Reply, String>,
    action: bool,
}
pub struct Worker {
    pub results: Receiver<Update>,
    control: SyncSender<Request>,
}
impl Worker {
    pub fn start() -> std::io::Result<Self> {
        let (sender, results) = mpsc::sync_channel(4);
        let (control, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("agx-services".into())
            .spawn(move || {
                let mut cursor = 0;
                loop {
                    let reply = agx_init::request(&Request::Logs {
                        service: None,
                        after: cursor,
                    })
                    .map_err(|e| e.to_string());
                    if let Ok(reply) = &reply {
                        cursor = if reply.last_sequence < cursor {
                            0
                        } else {
                            reply.last_sequence
                        };
                    }
                    if sender
                        .send(Update {
                            reply,
                            action: false,
                        })
                        .is_err()
                    {
                        break;
                    }
                    match receiver.recv_timeout(Duration::from_secs(1)) {
                        Ok(command) => {
                            let reply = agx_init::request(&command).map_err(|e| e.to_string());
                            if sender
                                .send(Update {
                                    reply,
                                    action: true,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => (),
                    }
                }
            })?;
        Ok(Self { results, control })
    }
    pub fn send(&self, command: Request, ui: &mut Ui) {
        if self.control.try_send(command).is_ok() {
            ui.busy = true;
            ui.notice = "Requesting operation...".into();
        }
    }
}
#[derive(Default)]
pub struct Ui {
    reply: Option<Reply>,
    logs: VecDeque<agx_init::Log>,
    sequence: u64,
    table: TableState,
    table_area: Rect,
    start_button: Rect,
    stop_button: Rect,
    restart_button: Rect,
    confirm_button: Rect,
    cancel_button: Rect,
    filter_button: Rect,
    pending: Option<(String, Action)>,
    confirm: bool,
    dialog_usable: bool,
    service_filter: Option<String>,
    text_filter: String,
    editing_filter: bool,
    log_scroll: usize,
    busy: bool,
    notice: String,
    error: Option<String>,
    last_success: Option<Instant>,
}
impl Ui {
    pub fn update(&mut self, update: Update) {
        match update.reply {
            Ok(reply) => {
                if reply.last_sequence < self.sequence {
                    self.logs.clear();
                    self.sequence = 0;
                }
                for entry in &reply.logs {
                    if entry.sequence > self.sequence {
                        self.logs.push_back(entry.clone());
                        self.sequence = entry.sequence;
                    }
                }
                while self.logs.len() > 512 {
                    self.logs.pop_front();
                }
                if reply.logs_truncated {
                    self.notice = "Older journal events expired (512-event limit).".into();
                }
                if update.action {
                    self.busy = false;
                    self.notice = reply.message.clone();
                }
                self.error = if reply.ok {
                    None
                } else {
                    Some(reply.message.clone())
                };
                self.last_success = Some(Instant::now());
                self.reply = Some(reply);
                let count = self.reply.as_ref().unwrap().services.len();
                self.table.select(if count == 0 {
                    None
                } else {
                    Some(self.table.selected().unwrap_or(0).min(count - 1))
                });
            }
            Err(error) => {
                self.error = Some(error);
                if update.action {
                    self.busy = false;
                }
            }
        }
    }
    pub fn move_row(&mut self, delta: i32, logs: bool) {
        if logs {
            self.log_scroll = self
                .log_scroll
                .saturating_add_signed(-delta as isize)
                .min(512);
        } else if let Some(reply) = &self.reply
            && !reply.services.is_empty()
        {
            self.table.select(Some(
                (self.table.selected().unwrap_or(0) as i32 + delta)
                    .clamp(0, reply.services.len() as i32 - 1) as usize,
            ));
        }
    }
    pub fn popup(&self) -> bool {
        self.pending.is_some() || self.editing_filter
    }
    fn begin(&mut self, action: Action) -> Option<Request> {
        if self.busy {
            return None;
        }
        let service = self.reply.as_ref()?.services.get(self.table.selected()?)?;
        if !service.controllable {
            self.notice = "Recovery service is protected.".into();
            return None;
        }
        if matches!(action, Action::Start) {
            return Some(Request::Control {
                name: service.name.clone(),
                action,
                confirmed: false,
            });
        }
        self.pending = Some((service.name.clone(), action));
        self.confirm = false;
        None
    }
    fn cycle_filter(&mut self) {
        let mut filters = vec![None, Some("init".to_owned())];
        if let Some(reply) = &self.reply {
            filters.extend(reply.services.iter().map(|s| Some(s.name.clone())));
        }
        let at = filters
            .iter()
            .position(|f| f == &self.service_filter)
            .unwrap_or(0);
        self.service_filter = filters[(at + 1) % filters.len()].clone();
        self.log_scroll = 0;
    }
    pub fn event(&mut self, event: &Event, logs: bool) -> Option<Request> {
        if self.editing_filter {
            if let Event::Key(key) = event {
                match key.code {
                    KeyCode::Enter | KeyCode::Esc => self.editing_filter = false,
                    KeyCode::Backspace => {
                        self.text_filter.pop();
                    }
                    KeyCode::Char(c) if !c.is_control() && self.text_filter.len() < 64 => {
                        self.text_filter.push(c)
                    }
                    _ => (),
                }
            }
            return None;
        }
        if self.pending.is_some() {
            let mut submit = false;
            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Esc | KeyCode::Char('n') => {
                        self.pending = None;
                    }
                    KeyCode::Tab | KeyCode::Left | KeyCode::Right => self.confirm = !self.confirm,
                    KeyCode::Enter => {
                        submit = self.confirm;
                        if !self.confirm {
                            self.pending = None;
                        }
                    }
                    KeyCode::Char('y') => {
                        self.confirm = true;
                        submit = true;
                    }
                    _ => (),
                },
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    if self
                        .cancel_button
                        .contains((mouse.column, mouse.row).into())
                    {
                        self.pending = None;
                    }
                    if self
                        .confirm_button
                        .contains((mouse.column, mouse.row).into())
                    {
                        self.confirm = true;
                        submit = true;
                    }
                }
                _ => (),
            }
            if submit
                && self.dialog_usable
                && let Some((name, action)) = self.pending.take()
            {
                return Some(Request::Control {
                    name,
                    action,
                    confirmed: true,
                });
            }
            return None;
        }
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Char('s') if !logs => return self.begin(Action::Start),
                KeyCode::Char('x') if !logs => return self.begin(Action::Stop),
                KeyCode::Char('t') if !logs => return self.begin(Action::Restart),
                KeyCode::Char('f') if logs => self.cycle_filter(),
                KeyCode::Char('/') if logs => self.editing_filter = true,
                KeyCode::Home if logs => self.log_scroll = 512,
                KeyCode::End if logs => self.log_scroll = 0,
                _ => (),
            },
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                let point = (mouse.column, mouse.row).into();
                if logs {
                    if self.filter_button.contains(point) {
                        self.cycle_filter();
                    }
                } else if self.start_button.contains(point) {
                    return self.begin(Action::Start);
                } else if self.stop_button.contains(point) {
                    return self.begin(Action::Stop);
                } else if self.restart_button.contains(point) {
                    return self.begin(Action::Restart);
                } else if self.table_area.contains(point) && mouse.row > self.table_area.y {
                    let row = self.table.offset() + (mouse.row - self.table_area.y - 1) as usize;
                    if self
                        .reply
                        .as_ref()
                        .is_some_and(|reply| row < reply.services.len())
                    {
                        self.table.select(Some(row));
                    }
                }
            }
            _ => (),
        }
        None
    }
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        p: Palette,
        options: Options,
        logs: bool,
    ) {
        let block = p.block(options, if logs { " Logs " } else { " Services " }, p.cyan);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let regions = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);
        let status = if let Some(error) = &self.error {
            format!("STALE / UNAVAILABLE: {error}")
        } else if self.last_success.is_none() {
            "Loading service manager...".into()
        } else if self.reply.as_ref().is_some_and(|r| r.pid1) {
            "Rust PID 1 / live process state".into()
        } else {
            "Rust supervisor / BusyBox recovery init".into()
        };
        frame.render_widget(
            Paragraph::new(status)
                .style(Style::default().fg(if self.error.is_some() {
                    p.amber
                } else {
                    p.green
                }))
                .wrap(Wrap { trim: false }),
            regions[0],
        );
        if logs {
            self.filter_button = Rect::new(regions[1].x, regions[1].y, regions[1].width, 1);
            let heading = format!(
                "f Filter: {} | / Search: {}{}",
                self.service_filter.as_deref().unwrap_or("all"),
                self.text_filter,
                if self.editing_filter {
                    "_ (Enter done)"
                } else {
                    ""
                }
            );
            let rows: Vec<_> = self
                .logs
                .iter()
                .filter(|entry| {
                    self.service_filter
                        .as_ref()
                        .is_none_or(|s| s == &entry.service)
                        && (self.text_filter.is_empty()
                            || entry
                                .message
                                .to_lowercase()
                                .contains(&self.text_filter.to_lowercase()))
                })
                .collect();
            let height = regions[1].height.saturating_sub(1) as usize;
            let end = rows
                .len()
                .saturating_sub(self.log_scroll.min(rows.len().saturating_sub(height)));
            let start = end.saturating_sub(height);
            let mut lines = vec![Line::styled(heading, Style::default().fg(p.cyan))];
            if rows.is_empty() {
                lines.push(Line::raw("No matching lifecycle events."));
            } else {
                lines.extend(rows[start..end].iter().map(|entry| {
                    Line::raw(format!(
                        "{} {}: {}",
                        entry.unix_seconds, entry.service, entry.message
                    ))
                }));
            }
            frame.render_widget(Paragraph::new(lines), regions[1]);
            frame.render_widget(
                Paragraph::new(
                    "Lifecycle journal: latest 512 events this boot. Up/Down scroll; End follows.",
                )
                .style(Style::default().fg(p.muted))
                .wrap(Wrap { trim: false }),
                regions[2],
            );
        } else {
            let rows = self
                .reply
                .as_ref()
                .map(|r| {
                    r.services
                        .iter()
                        .map(|s| {
                            Row::new(vec![
                                Cell::from(s.name.clone()),
                                Cell::from(s.state.clone()),
                                Cell::from(s.pid.map_or("-".into(), |pid| pid.to_string())),
                                Cell::from(s.starts.to_string()),
                            ])
                            .style(Style::default().fg(
                                if s.state == "running" {
                                    p.green
                                } else {
                                    p.amber
                                },
                            ))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            self.table_area = regions[1];
            let table = Table::new(
                rows,
                [
                    Constraint::Percentage(35),
                    Constraint::Percentage(30),
                    Constraint::Percentage(20),
                    Constraint::Percentage(15),
                ],
            )
            .header(
                Row::new(["SERVICE", "STATE", "PID", "STARTS"])
                    .style(Style::default().fg(p.purple)),
            )
            .row_highlight_style(Style::default().bg(p.bg).bold())
            .highlight_symbol(if options.ascii { "> " } else { "› " });
            frame.render_stateful_widget(table, regions[1], &mut self.table);
            let controls = "[s Start]  [x Stop]  [t Restart]";
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(controls, Style::default().fg(p.cyan)),
                    Line::raw(self.notice.clone()),
                ]),
                regions[2],
            );
            self.start_button = Rect::new(regions[2].x, regions[2].y, 9.min(regions[2].width), 1);
            self.stop_button = Rect::new(regions[2].x + 11, regions[2].y, 8, 1);
            self.restart_button = Rect::new(regions[2].x + 21, regions[2].y, 11, 1);
        }
        if let Some((name, action)) = &self.pending {
            let screen = frame.area();
            self.dialog_usable = screen.width >= 50 && screen.height >= 14;
            let rect = Rect::new(
                screen.width.saturating_sub(66) / 2,
                screen.height.saturating_sub(10) / 2,
                screen.width.min(66),
                screen.height.min(10),
            );
            frame.render_widget(Clear, rect);
            let block = p.block(options, " Confirm service operation ", p.amber);
            let inside = block.inner(rect);
            frame.render_widget(block, rect);
            let text = if self.dialog_usable {
                format!(
                    "{action:?} {name}?\n\nNetwork operations briefly interrupt Wi-Fi.\nUSB SSH remains available.\n\n{}  {}\nTab changes choice; Enter selects; Esc cancels.",
                    if self.confirm {
                        "[ Cancel ]"
                    } else {
                        "[> Cancel <]"
                    },
                    if self.confirm {
                        "[> Confirm <]"
                    } else {
                        "[ Confirm ]"
                    }
                )
            } else {
                "Resize to 50 x 14. Esc cancels.".into()
            };
            self.cancel_button = Rect::new(inside.x, inside.y + 5, 13, 1);
            self.confirm_button = Rect::new(inside.x + 14, inside.y + 5, 15, 1);
            frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inside);
        }
    }
}
