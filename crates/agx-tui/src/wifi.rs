//! Wi-Fi provisioning views and asynchronous service client.
use super::{Options, Palette};
use agx_net::{Request, Security, Status};
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};
use std::{
    io,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};
use zeroize::Zeroizing;

pub(super) struct Update {
    pub power_accepted: Option<bool>,
    status: Option<Status>,
    feedback: Option<Result<String, String>>,
    error: Option<String>,
}
pub(super) struct Worker {
    pub results: Receiver<Update>,
    commands: SyncSender<Request>,
}
impl Worker {
    pub fn start() -> io::Result<Self> {
        let (sender, results) = mpsc::sync_channel(8);
        let (commands, receiver) = mpsc::sync_channel(2);
        std::thread::Builder::new()
            .name("agx-network".into())
            .spawn(move || {
                let mut action = None;
                loop {
                    let command = action.take().unwrap_or(Request::Status);
                    let interactive = !matches!(command, Request::Status);
                    let power = match command {
                        Request::Reboot { confirmed: true } => Some(true),
                        Request::Shutdown { confirmed: true } => Some(false),
                        _ => None,
                    };
                    let update = match agx_net::request(&command) {
                        Ok(reply) => Update {
                            power_accepted: power.filter(|_| reply.ok),
                            status: Some(reply.status),
                            feedback: if interactive {
                                Some(if reply.ok {
                                    Ok(reply.message)
                                } else {
                                    Err(reply.message)
                                })
                            } else {
                                None
                            },
                            error: None,
                        },
                        Err(error) => Update {
                            power_accepted: None,
                            status: None,
                            feedback: if interactive {
                                Some(Err(error.to_string()))
                            } else {
                                None
                            },
                            error: Some(error.to_string()),
                        },
                    };
                    if sender.send(update).is_err() {
                        break;
                    }
                    match receiver.recv_timeout(Duration::from_secs(1)) {
                        Ok(request) => action = Some(request),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => (),
                    }
                }
            })?;
        Ok(Self { results, commands })
    }
    pub fn send(&self, command: Request, ui: &mut Ui) {
        if self.commands.try_send(command).is_err() {
            ui.notice = Some("Another operation is pending; try again shortly".into());
        } else {
            ui.notice = Some("Request sent...".into());
        }
    }
}
pub(super) enum Interaction {
    Unused,
    Consumed,
    Send(Request),
}
struct ConnectForm {
    ssid: String,
    ssid_hex: String,
    security: Security,
    password: Zeroizing<String>,
    manual: bool,
    focus: usize,
    error: Option<String>,
}
enum Confirm {
    Reboot,
    Shutdown,
    Disconnect,
    Forget { id: i32, ssid: String },
}
enum Form {
    Connect(ConnectForm),
    Country {
        value: String,
        focus: usize,
        error: Option<String>,
    },
    Confirm {
        kind: Confirm,
        focus: usize,
    },
}
#[derive(Default)]
pub(super) struct Ui {
    status: Option<Status>,
    error: Option<String>,
    pub notice: Option<String>,
    table: TableState,
    saved: bool,
    pub reboot_area: Rect,
    pub shutdown_area: Rect,
    list_area: Rect,
    buttons: Vec<Rect>,
    popup: Option<Form>,
    fields: Vec<Rect>,
    popup_buttons: Vec<Rect>,
    can_submit: bool,
}

impl Ui {
    pub fn update(&mut self, update: Update) {
        let key = self.table.selected().and_then(|i| {
            self.status.as_ref().and_then(|s| {
                if self.saved {
                    s.saved.get(i).map(|n| n.ssid_hex.clone())
                } else {
                    s.access_points.get(i).map(|n| n.ssid_hex.clone())
                }
            })
        });
        if let Some(status) = update.status {
            self.status = Some(status);
            self.error = None;
            if let Some(key) = key {
                let position = self.status.as_ref().and_then(|s| {
                    if self.saved {
                        s.saved.iter().position(|n| n.ssid_hex == key)
                    } else {
                        s.access_points.iter().position(|n| n.ssid_hex == key)
                    }
                });
                self.table.select(position);
            }
            let count = self.count();
            self.table.select(if count == 0 {
                None
            } else {
                Some(self.table.selected().unwrap_or(0).min(count - 1))
            });
        }
        if let Some(error) = update.error {
            self.error = Some(error);
        }
        if let Some(feedback) = update.feedback {
            self.notice = Some(match feedback {
                Ok(message) => message,
                Err(error) => format!("Error: {error}"),
            });
        }
    }
    pub fn entered(&mut self) -> Option<Request> {
        if self
            .status
            .as_ref()
            .is_some_and(|s| s.access_points.is_empty() && !s.scanning && !s.busy)
        {
            Some(Request::Scan)
        } else {
            None
        }
    }
    fn count(&self) -> usize {
        self.status.as_ref().map_or(0, |s| {
            if self.saved {
                s.saved.len()
            } else {
                s.access_points.len()
            }
        })
    }
    pub fn move_row(&mut self, delta: i32) {
        let count = self.count();
        self.table.select(if count == 0 {
            None
        } else {
            Some(
                (self.table.selected().unwrap_or(0) as i32 + delta).clamp(0, count as i32 - 1)
                    as usize,
            )
        });
    }
    pub fn home_end(&mut self, end: bool) {
        let count = self.count();
        self.table.select(if count == 0 {
            None
        } else if end {
            Some(count - 1)
        } else {
            Some(0)
        });
    }
    pub fn has_popup(&self) -> bool {
        self.popup.is_some()
    }
    pub fn reboot(&mut self) {
        self.popup = Some(Form::Confirm {
            kind: Confirm::Reboot,
            focus: 0,
        });
    }
    pub fn shutdown(&mut self) {
        self.popup = Some(Form::Confirm {
            kind: Confirm::Shutdown,
            focus: 0,
        });
    }
    fn connect(&mut self) -> Interaction {
        let Some(status) = &self.status else {
            self.notice = Some("Wi-Fi service is not ready".into());
            return Interaction::Consumed;
        };
        let index = self.table.selected().unwrap_or(0);
        if self.saved {
            return status.saved.get(index).map_or(Interaction::Consumed, |n| {
                Interaction::Send(Request::SelectSaved { id: n.id })
            });
        }
        let Some(network) = status.access_points.get(index) else {
            return Interaction::Consumed;
        };
        if network.security == Security::Unsupported {
            self.notice=Some("This network requires Enterprise or WEP authentication, which is not supported yet.".into());
            return Interaction::Consumed;
        }
        self.popup = Some(Form::Connect(ConnectForm {
            ssid: network.ssid.clone(),
            ssid_hex: network.ssid_hex.clone(),
            security: network.security,
            password: Zeroizing::new(String::new()),
            manual: false,
            focus: if matches!(network.security, Security::Open | Security::Owe) {
                4
            } else {
                2
            },
            error: None,
        }));
        Interaction::Consumed
    }
    fn action(&mut self, index: usize) -> Interaction {
        match index {
            0 => Interaction::Send(Request::Scan),
            1 => {
                self.popup = Some(Form::Connect(ConnectForm {
                    ssid: String::new(),
                    ssid_hex: String::new(),
                    security: Security::Wpa2,
                    password: Zeroizing::new(String::new()),
                    manual: true,
                    focus: 0,
                    error: None,
                }));
                Interaction::Consumed
            }
            2 => {
                self.popup = Some(Form::Country {
                    value: self
                        .status
                        .as_ref()
                        .map_or("US", |s| s.country.as_str())
                        .into(),
                    focus: 0,
                    error: None,
                });
                Interaction::Consumed
            }
            3 => {
                self.popup = Some(Form::Confirm {
                    kind: Confirm::Disconnect,
                    focus: 0,
                });
                Interaction::Consumed
            }
            4 => Interaction::Send(Request::Check),
            5 => {
                self.saved = !self.saved;
                self.table.select(Some(0));
                Interaction::Consumed
            }
            _ => Interaction::Consumed,
        }
    }
    pub fn key(&mut self, key: KeyEvent) -> Interaction {
        match key.code {
            KeyCode::Char('s') => self.action(0),
            KeyCode::Char('h') => self.action(1),
            KeyCode::Char('c') => self.action(2),
            KeyCode::Char('d') => self.action(3),
            KeyCode::Char('i') => self.action(4),
            KeyCode::Char('v') => self.action(5),
            KeyCode::Enter => self.connect(),
            KeyCode::Char('f') if self.saved => {
                if let Some(network) = self
                    .table
                    .selected()
                    .and_then(|i| self.status.as_ref()?.saved.get(i))
                {
                    self.popup = Some(Form::Confirm {
                        kind: Confirm::Forget {
                            id: network.id,
                            ssid: network.ssid.clone(),
                        },
                        focus: 0,
                    });
                }
                Interaction::Consumed
            }
            _ => Interaction::Unused,
        }
    }
    pub fn mouse(&mut self, event: ratatui::crossterm::event::MouseEvent) -> Interaction {
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(index) = self
                    .buttons
                    .iter()
                    .position(|r| r.contains((event.column, event.row).into()))
                {
                    return self.action(index);
                }
                if self.list_area.contains((event.column, event.row).into())
                    && event.row >= self.list_area.y + 2
                    && event.row < self.list_area.bottom().saturating_sub(1)
                {
                    let index = self.table.offset() + (event.row - self.list_area.y - 2) as usize;
                    if index < self.count() {
                        self.table.select(Some(index));
                        return self.connect();
                    }
                }
            }
            MouseEventKind::ScrollDown => self.move_row(3),
            MouseEventKind::ScrollUp => self.move_row(-3),
            _ => (),
        }
        Interaction::Consumed
    }
    pub fn draw_shutdown(&mut self, frame: &mut Frame, area: Rect, p: Palette) {
        self.shutdown_area = area;
        frame.render_widget(
            Paragraph::new("[Shutdown F9]").style(Style::default().fg(p.cyan)),
            area,
        );
    }
    pub fn draw_reboot(&mut self, frame: &mut Frame, area: Rect, p: Palette) {
        self.reboot_area = area;
        frame.render_widget(
            Paragraph::new("[Reboot F10]")
                .centered()
                .style(Style::default().fg(p.amber).bold()),
            area,
        );
    }
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, p: Palette, options: Options) {
        self.buttons.clear();
        self.list_area = Rect::default();
        let Some(status) = &self.status else {
            frame.render_widget(
                Paragraph::new(
                    self.error
                        .as_deref()
                        .unwrap_or("Connecting to Wi-Fi provisioning service..."),
                )
                .wrap(Wrap { trim: false })
                .block(block(" Wi-Fi provisioning ", p, options)),
                area,
            );
            return;
        };
        let feedback = self
            .notice
            .as_ref()
            .or(status.error.as_ref())
            .map(String::as_str)
            .unwrap_or(&status.message);
        if area.height < 14 || area.width < 60 {
            let selected = self
                .table
                .selected()
                .and_then(|i| {
                    if self.saved {
                        status.saved.get(i).map(|n| n.ssid.as_str())
                    } else {
                        status.access_points.get(i).map(|n| n.ssid.as_str())
                    }
                })
                .unwrap_or("No networks listed");
            let text = format!(
                "{} / {}\n{}\nSelected: {selected}\ns scan, arrows select, Enter connect\nh hidden / c country / v saved\nResize to 60x20 for dialogs",
                status.phase,
                status.country,
                status.ssid.as_deref().unwrap_or("Not associated")
            );
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: false }).block(block(
                    " Wi-Fi / compact ",
                    p,
                    options,
                )),
                area,
            );
            return;
        }
        let parts = Layout::vertical([
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);
        let color = if self.error.is_some() {
            p.red
        } else if status.phase == "Connected" {
            p.green
        } else {
            p.amber
        };
        let lines = vec![
            Line::from(vec![
                Span::styled(
                    format!(
                        "{}  ",
                        if self.error.is_some() {
                            "Service unavailable"
                        } else {
                            &status.phase
                        }
                    ),
                    Style::default().fg(color).bold(),
                ),
                Span::raw(
                    status
                        .ssid
                        .as_deref()
                        .unwrap_or("Choose a network to connect"),
                ),
            ]),
            Line::raw(format!(
                "Region: {}  |  IP: {}  |  Signal: {}",
                status.country,
                if status.ipv4.is_empty() {
                    "pending".into()
                } else {
                    status.ipv4.join(", ")
                },
                status
                    .signal_dbm
                    .map_or("--".into(), |n| format!("{n} dBm"))
            )),
            Line::raw(format!(
                "Gateway: {}  |  DNS: {}  |  Clock: {}",
                status.gateway.as_deref().unwrap_or("--"),
                status.dns.join(", "),
                if status.time_synced {
                    "synced"
                } else {
                    "waiting for sync"
                }
            )),
            Line::styled(
                self.error.as_deref().unwrap_or(feedback).to_owned(),
                Style::default().fg(
                    if self.error.is_some()
                        || status.error.is_some()
                        || feedback.starts_with("Error:")
                    {
                        p.red
                    } else {
                        p.muted
                    },
                ),
            ),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block(" NETWORK PROVISIONING ", p, options)),
            parts[0],
        );
        let buttons = Layout::horizontal([Constraint::Ratio(1, 6); 6]).split(parts[1]);
        self.buttons = buttons.to_vec();
        for (index, label) in [
            "Scan S",
            "Hidden H",
            "Region C",
            "Offline D",
            "Check I",
            if self.saved { "Nearby V" } else { "Saved V" },
        ]
        .into_iter()
        .enumerate()
        {
            frame.render_widget(
                Paragraph::new(label)
                    .centered()
                    .style(Style::default().fg(p.cyan).bold())
                    .block(block("", p, options)),
                buttons[index],
            );
        }
        self.list_area = parts[2];
        let rows: Vec<Row> = if self.saved {
            status
                .saved
                .iter()
                .map(|n| {
                    Row::new(vec![
                        Cell::from(n.ssid.clone()),
                        Cell::from(if n.current { "current" } else { "saved" }),
                        Cell::from("authentication saved"),
                        Cell::from(""),
                    ])
                })
                .collect()
        } else {
            status
                .access_points
                .iter()
                .map(|n| {
                    Row::new(vec![
                        Cell::from(n.ssid.clone()),
                        Cell::from(format!("{} dBm", n.signal_dbm)).style(Style::default().fg(
                            if n.signal_dbm >= -60 {
                                p.green
                            } else {
                                p.amber
                            },
                        )),
                        Cell::from(n.security.label()),
                        Cell::from(if status.saved.iter().any(|s| s.ssid_hex == n.ssid_hex) {
                            "yes"
                        } else {
                            ""
                        }),
                    ])
                })
                .collect()
        };
        let title = if self.saved {
            format!(" SAVED NETWORKS / {} ", rows.len())
        } else {
            format!(
                " NEARBY NETWORKS / {} {} ",
                rows.len(),
                if status.scanning { "/ scanning..." } else { "" }
            )
        };
        let table = Table::new(
            rows,
            [
                Constraint::Min(12),
                Constraint::Length(9),
                Constraint::Length(16),
                Constraint::Length(5),
            ],
        )
        .header(
            Row::new(if self.saved {
                ["NETWORK", "STATE", "DETAIL", ""]
            } else {
                ["NETWORK", "SIGNAL", "AUTHENTICATION", "SAVED"]
            })
            .style(Style::default().fg(p.cyan).bold()),
        )
        .row_highlight_style(Style::default().fg(p.cyan).bg(p.bg).bold())
        .highlight_symbol("> ")
        .block(block(&title, p, options));
        frame.render_stateful_widget(table, parts[2], &mut self.table);
        let hint = if self.saved {
            "Enter reconnect | f forget | v nearby networks"
        } else {
            "Click a network or press Enter to authenticate and save"
        };
        frame.render_widget(
            Paragraph::new(format!("{hint}\nInternet check: {}", status.diagnostics))
                .style(Style::default().fg(p.muted)),
            parts[3],
        );
    }
    pub fn draw_popup(&mut self, frame: &mut Frame, p: Palette, options: Options) {
        let Some(form) = &self.popup else {
            self.can_submit = false;
            return;
        };
        let area = frame.area();
        self.fields.clear();
        self.popup_buttons.clear();
        self.can_submit = area.width >= 60 && area.height >= 20;
        if !self.can_submit {
            frame.render_widget(Clear, area);
            frame.render_widget(Paragraph::new("Provisioning dialog needs 60 columns x 20 rows. Resize the terminal; Esc cancels.").wrap(Wrap{trim:true}).block(block(" Resize terminal ",p,options)),area);
            return;
        }
        let height = match form {
            Form::Connect(_) => 18,
            Form::Country { .. } => 10,
            Form::Confirm { .. } => 10,
        };
        let width = area.width.min(76) - 4;
        let popup = Rect::new(
            (area.width - width) / 2,
            (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, popup);
        let title = match form {
            Form::Connect(_) => " Wi-Fi authentication ",
            Form::Country { .. } => " Radio region ",
            Form::Confirm {
                kind: Confirm::Reboot,
                ..
            } => " Reboot Pi ",
            Form::Confirm {
                kind: Confirm::Shutdown,
                ..
            } => " Power off Pi ",
            Form::Confirm {
                kind: Confirm::Disconnect,
                ..
            } => " Disconnect Wi-Fi ",
            _ => " Forget saved network ",
        };
        let border = block(title, p, options);
        let inner = border.inner(popup);
        frame.render_widget(border, popup);
        match form {
            Form::Connect(form) => {
                let rows = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(2),
                    Constraint::Length(3),
                ])
                .split(inner);
                frame.render_widget(
                    Paragraph::new("Credentials are saved on this Pi for automatic reconnect.")
                        .style(Style::default().fg(p.muted)),
                    rows[0],
                );
                self.fields = vec![rows[1], rows[2], rows[3]];
                field(
                    frame,
                    rows[1],
                    "Network name",
                    &form.ssid,
                    form.focus == 0,
                    p,
                    options,
                );
                field(
                    frame,
                    rows[2],
                    if form.manual {
                        "Authentication / Space changes"
                    } else {
                        "Authentication"
                    },
                    form.security.label(),
                    form.focus == 1,
                    p,
                    options,
                );
                let password = if matches!(form.security, Security::Open | Security::Owe) {
                    "Not required".into()
                } else {
                    "*".repeat(form.password.chars().count())
                };
                field(
                    frame,
                    rows[3],
                    "Wi-Fi password",
                    &password,
                    form.focus == 2,
                    p,
                    options,
                );
                let note = form
                    .error
                    .as_deref()
                    .unwrap_or(if form.security == Security::Open {
                        "Open network: wireless traffic is not encrypted."
                    } else {
                        "Tab changes fields. Enter connects and saves. Esc cancels."
                    });
                frame.render_widget(
                    Paragraph::new(note).wrap(Wrap { trim: true }).style(
                        Style::default().fg(if form.error.is_some() { p.red } else { p.amber }),
                    ),
                    rows[4],
                );
                self.popup_buttons = Layout::horizontal([Constraint::Ratio(1, 2); 2])
                    .split(rows[5])
                    .to_vec();
                button(
                    frame,
                    self.popup_buttons[0],
                    "Cancel",
                    form.focus == 3,
                    p,
                    options,
                );
                button(
                    frame,
                    self.popup_buttons[1],
                    "Connect & save",
                    form.focus == 4,
                    p,
                    options,
                );
            }
            Form::Country {
                value,
                focus,
                error,
            } => {
                let rows = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(3),
                ])
                .split(inner);
                frame.render_widget(
                    Paragraph::new("Use the country where this Pi operates."),
                    rows[0],
                );
                self.fields = vec![rows[1]];
                field(
                    frame,
                    rows[1],
                    "Two-letter country code",
                    value,
                    *focus == 0,
                    p,
                    options,
                );
                frame.render_widget(
                    Paragraph::new(error.clone().unwrap_or_default())
                        .style(Style::default().fg(p.red)),
                    rows[2],
                );
                self.popup_buttons = Layout::horizontal([Constraint::Ratio(1, 2); 2])
                    .split(rows[3])
                    .to_vec();
                button(
                    frame,
                    self.popup_buttons[0],
                    "Cancel",
                    *focus == 1,
                    p,
                    options,
                );
                button(
                    frame,
                    self.popup_buttons[1],
                    "Save region",
                    *focus == 2,
                    p,
                    options,
                );
            }
            Form::Confirm { kind, focus } => {
                let rows =
                    Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(inner);
                let message=match kind{Confirm::Shutdown=>"Power off this Pi now?\nActive SSH sessions will close. Wait before unplugging power.".into(),Confirm::Reboot=>"Reboot this Pi now?\nActive SSH sessions will close. Saved settings are retained.".into(),Confirm::Disconnect=>"Disconnect Wi-Fi?\nWi-Fi SSH sessions may close. USB SSH stays available.".into(),Confirm::Forget{ssid,..}=>format!("Forget {ssid}?\nIts saved authentication will be removed. An active connection to it will close.")};
                frame.render_widget(
                    Paragraph::new(message)
                        .wrap(Wrap { trim: false })
                        .style(Style::default().fg(p.amber)),
                    rows[0],
                );
                self.popup_buttons = Layout::horizontal([Constraint::Ratio(1, 2); 2])
                    .split(rows[1])
                    .to_vec();
                button(
                    frame,
                    self.popup_buttons[0],
                    "Cancel",
                    *focus == 0,
                    p,
                    options,
                );
                button(
                    frame,
                    self.popup_buttons[1],
                    match kind {
                        Confirm::Reboot => "Reboot now",
                        Confirm::Shutdown => "Power off",
                        Confirm::Disconnect => "Disconnect",
                        _ => "Forget",
                    },
                    *focus == 1,
                    p,
                    options,
                );
            }
        }
    }
    pub fn popup_event(&mut self, event: &Event) -> Interaction {
        if matches!(event,Event::Key(key) if key.code==KeyCode::Esc) {
            self.popup = None;
            return Interaction::Consumed;
        }
        if !self.can_submit {
            return Interaction::Consumed;
        }
        if let Event::Paste(value) = event {
            match self.popup.as_mut() {
                Some(Form::Connect(form)) => {
                    if value.chars().any(char::is_control) || value.len() > 128 {
                        form.error=Some("Paste contains line breaks or is too long. Paste only the field value.".into());
                    } else if form.focus == 0 && form.manual && form.ssid.len() + value.len() <= 32
                    {
                        form.ssid.push_str(value);
                    } else if form.focus == 2 && form.password.len() + value.len() <= 128 {
                        form.password.push_str(value);
                    }
                }
                Some(Form::Country {
                    value: country,
                    focus,
                    error,
                }) if *focus == 0 => {
                    if value.len() <= 2 && value.bytes().all(|b| b.is_ascii_alphabetic()) {
                        *country = value.to_ascii_uppercase();
                    } else {
                        *error = Some("Paste a two-letter country code".into());
                    }
                }
                _ => (),
            }
            return Interaction::Consumed;
        }
        let mut submit = false;
        let mut cancel = false;
        let clicked = if let Event::Mouse(mouse) = event {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                self.popup_buttons
                    .iter()
                    .position(|r| r.contains((mouse.column, mouse.row).into()))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(index) = clicked {
            cancel = index == 0;
            submit = index == 1;
        }
        let key = if let Event::Key(key) = event {
            Some(key)
        } else {
            None
        };
        match self.popup.as_mut() {
            Some(Form::Connect(form)) => {
                if let Event::Mouse(mouse) = event
                    && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && let Some(index) = self
                        .fields
                        .iter()
                        .position(|r| r.contains((mouse.column, mouse.row).into()))
                {
                    form.focus = index;
                }
                if let Some(key) = key {
                    match key.code {
                        KeyCode::Tab => form.focus = (form.focus + 1) % 5,
                        KeyCode::BackTab => form.focus = (form.focus + 4) % 5,
                        KeyCode::Enter => {
                            if form.focus == 3 {
                                cancel = true
                            } else {
                                submit = true
                            }
                        }
                        KeyCode::Backspace => {
                            if form.focus == 0 && form.manual {
                                form.ssid.pop();
                            }
                            if form.focus == 2 {
                                form.password.pop();
                            }
                        }
                        KeyCode::Char(' ') if form.focus == 1 && form.manual => {
                            form.security = match form.security {
                                Security::Wpa2 => Security::Wpa3,
                                Security::Wpa3 => Security::Open,
                                Security::Open => Security::Owe,
                                _ => Security::Wpa2,
                            }
                        }
                        KeyCode::Char(c) if !c.is_control() => {
                            if form.focus == 0
                                && form.manual
                                && form.ssid.len() + c.len_utf8() <= 32
                            {
                                form.ssid.push(c);
                            }
                            if form.focus == 2 && form.password.len() + c.len_utf8() <= 128 {
                                form.password.push(c);
                            }
                        }
                        _ => (),
                    }
                }
                if submit {
                    let ssid_hex = if form.manual {
                        agx_net::hex(form.ssid.as_bytes())
                    } else {
                        form.ssid_hex.clone()
                    };
                    match agx_net::validate_credentials(&ssid_hex, form.security, &form.password) {
                        Ok(()) => {
                            let request = Request::Connect {
                                ssid_hex,
                                security: form.security,
                                password: std::mem::take(&mut form.password),
                                hidden: form.manual,
                            };
                            self.popup = None;
                            return Interaction::Send(request);
                        }
                        Err(error) => form.error = Some(error),
                    }
                }
            }
            Some(Form::Country {
                value,
                focus,
                error,
            }) => {
                if let Event::Mouse(mouse) = event
                    && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && self
                        .fields
                        .iter()
                        .any(|r| r.contains((mouse.column, mouse.row).into()))
                {
                    *focus = 0;
                }

                if let Some(key) = key {
                    match key.code {
                        KeyCode::Tab => *focus = (*focus + 1) % 3,
                        KeyCode::BackTab => *focus = (*focus + 2) % 3,
                        KeyCode::Backspace if *focus == 0 => {
                            value.pop();
                        }
                        KeyCode::Char(c)
                            if *focus == 0 && c.is_ascii_alphabetic() && value.len() < 2 =>
                        {
                            value.push(c.to_ascii_uppercase())
                        }
                        KeyCode::Enter => {
                            if *focus == 1 {
                                cancel = true
                            } else {
                                submit = true
                            }
                        }
                        _ => (),
                    }
                }
                if submit {
                    if value.len() == 2 {
                        let request = Request::Country {
                            country: value.clone(),
                        };
                        self.popup = None;
                        return Interaction::Send(request);
                    } else {
                        *error = Some("Enter two letters, such as US".into());
                    }
                }
            }
            Some(Form::Confirm { kind, focus }) => {
                if let Some(key) = key {
                    match key.code {
                        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                            *focus = 1 - *focus
                        }
                        KeyCode::Enter => {
                            if *focus == 0 {
                                cancel = true
                            } else {
                                submit = true
                            }
                        }
                        _ => (),
                    }
                }
                if submit {
                    let request = match kind {
                        Confirm::Reboot => Request::Reboot { confirmed: true },
                        Confirm::Shutdown => Request::Shutdown { confirmed: true },
                        Confirm::Disconnect => Request::Disconnect,
                        Confirm::Forget { id, .. } => Request::Forget { id: *id },
                    };
                    self.popup = None;
                    return Interaction::Send(request);
                }
            }
            None => return Interaction::Unused,
        }
        if cancel {
            self.popup = None;
        }
        Interaction::Consumed
    }
}
fn block<'a>(title: &'a str, p: Palette, options: Options) -> Block<'a> {
    let mut block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.muted))
        .title_style(Style::default().fg(p.cyan).bold())
        .style(Style::default().bg(p.panel).fg(p.text));
    if options.ascii {
        block = block.border_set(ratatui::symbols::border::Set {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "|",
            vertical_right: "|",
            horizontal_top: "-",
            horizontal_bottom: "-",
        });
    } else {
        block = block.border_type(ratatui::widgets::BorderType::Rounded);
    }
    block
}
fn field(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    value: &str,
    focus: bool,
    p: Palette,
    options: Options,
) {
    let widget = block(title, p, options).border_style(Style::default().fg(if focus {
        p.cyan
    } else {
        p.muted
    }));
    let inner = widget.inner(area);
    frame.render_widget(Paragraph::new(value).block(widget), area);
    if focus && inner.width > 0 {
        frame.set_cursor_position((
            inner.x + (Line::from(value).width() as u16).min(inner.width - 1),
            inner.y,
        ));
    }
}
fn button(frame: &mut Frame, area: Rect, label: &str, focus: bool, p: Palette, options: Options) {
    frame.render_widget(
        Paragraph::new(if focus {
            format!("> {label} <")
        } else {
            label.into()
        })
        .centered()
        .style(
            Style::default()
                .fg(if focus { p.cyan } else { p.muted })
                .bold(),
        )
        .block(block("", p, options)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn password_is_masked_and_reboot_defaults_to_cancel() {
        let options = Options {
            ascii: true,
            color: super::super::ColorMode::Never,
            mouse: false,
        };
        let palette = Palette::new(options.color);
        let mut ui = Ui {
            popup: Some(Form::Connect(ConnectForm {
                ssid: "Test".into(),
                ssid_hex: agx_net::hex(b"Test"),
                security: Security::Wpa2,
                password: Zeroizing::new("never-print-this".into()),
                manual: false,
                focus: 2,
                error: None,
            })),
            ..Ui::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| ui.draw_popup(frame, palette, options))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(!text.contains("never-print-this"));
        assert!(text.contains("***************"));
        ui.reboot();
        terminal
            .draw(|frame| ui.draw_popup(frame, palette, options))
            .unwrap();
        assert!(matches!(
            ui.popup_event(&Event::Key(KeyCode::Enter.into())),
            Interaction::Consumed
        ));
        assert!(!ui.has_popup());
    }
}
