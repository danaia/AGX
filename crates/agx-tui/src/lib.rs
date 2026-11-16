//! Interactive AGX system console. Collection runs off the terminal input thread.
mod profile;
mod system;
mod wifi;
use agx_core::{Sampler, Snapshot};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{
            self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
            EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton,
            MouseEventKind,
        },
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, TableState, Wrap},
};
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
use wifi::Interaction;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorMode {
    Auto,
    TrueColor,
    Indexed,
    Basic,
    Never,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub color: ColorMode,
    pub ascii: bool,
    pub mouse: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            color: ColorMode::Auto,
            ascii: false,
            mouse: true,
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Color,
    panel: Color,
    text: Color,
    muted: Color,
    cyan: Color,
    purple: Color,
    green: Color,
    amber: Color,
    red: Color,
}
impl Palette {
    fn new(mode: ColorMode) -> Self {
        let mode = if mode == ColorMode::Auto {
            if std::env::var_os("NO_COLOR").is_some() {
                ColorMode::Never
            } else if std::env::var("COLORTERM")
                .is_ok_and(|s| s.contains("truecolor") || s.contains("24bit"))
            {
                ColorMode::TrueColor
            } else if std::env::var("TERM").is_ok_and(|s| s.contains("256color")) {
                ColorMode::Indexed
            } else {
                ColorMode::Basic
            }
        } else {
            mode
        };
        match mode {
            ColorMode::Never => Self {
                bg: Color::Reset,
                panel: Color::Reset,
                text: Color::Reset,
                muted: Color::Reset,
                cyan: Color::Reset,
                purple: Color::Reset,
                green: Color::Reset,
                amber: Color::Reset,
                red: Color::Reset,
            },
            ColorMode::TrueColor => Self {
                bg: Color::Rgb(10, 15, 26),
                panel: Color::Rgb(17, 25, 40),
                text: Color::Rgb(226, 232, 240),
                muted: Color::Rgb(136, 153, 177),
                cyan: Color::Rgb(103, 232, 249),
                purple: Color::Rgb(192, 167, 255),
                green: Color::Rgb(110, 231, 183),
                amber: Color::Rgb(251, 191, 36),
                red: Color::Rgb(251, 113, 133),
            },
            ColorMode::Indexed => Self {
                bg: Color::Indexed(233),
                panel: Color::Indexed(234),
                text: Color::Indexed(255),
                muted: Color::Indexed(246),
                cyan: Color::Indexed(87),
                purple: Color::Indexed(183),
                green: Color::Indexed(121),
                amber: Color::Indexed(221),
                red: Color::Indexed(210),
            },
            _ => Self {
                bg: Color::Reset,
                panel: Color::Reset,
                text: Color::White,
                muted: Color::Gray,
                cyan: Color::LightCyan,
                purple: Color::LightMagenta,
                green: Color::LightGreen,
                amber: Color::LightYellow,
                red: Color::LightRed,
            },
        }
    }
}

struct Session;
impl Session {
    fn enter(mouse: bool) -> io::Result<Self> {
        let session = Self;
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        if mouse {
            execute!(io::stdout(), EnableMouseCapture)?;
        }
        Ok(session)
    }
}
fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        ratatui::crossterm::cursor::Show
    );
    let _ = disable_raw_mode();
}
impl Drop for Session {
    fn drop(&mut self) {
        restore();
    }
}
struct Signals(Vec<signal_hook::SigId>);
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

struct Worker {
    results: Receiver<Result<Snapshot, String>>,
    refresh: SyncSender<()>,
}
impl Worker {
    fn start() -> io::Result<Self> {
        let (sender, results) = mpsc::sync_channel(1);
        let (refresh, control) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("agx-sampler".into())
            .spawn(move || {
                let mut sampler = Sampler::default();
                loop {
                    if let Err(mpsc::TrySendError::Disconnected(_)) =
                        sender.try_send(sampler.collect().map_err(|e| e.to_string()))
                    {
                        break;
                    }
                    if let Err(mpsc::RecvTimeoutError::Disconnected) =
                        control.recv_timeout(Duration::from_secs(1))
                    {
                        break;
                    }
                }
            })?;
        Ok(Self { results, refresh })
    }
}

struct App {
    page: usize,
    snapshot: Option<Snapshot>,
    error: Option<String>,
    last_success: Option<Instant>,
    scroll: u16,
    table: TableState,
    tabs: Vec<Rect>,
    table_area: Rect,
    options: Options,
    palette: Palette,
    wifi: wifi::Ui,
    profile: profile::Ui,
    system: system::Ui,
}
impl App {
    fn new(options: Options) -> Self {
        Self {
            page: 0,
            snapshot: None,
            error: None,
            last_success: None,
            scroll: 0,
            table: TableState::default(),
            tabs: Vec::new(),
            table_area: Rect::default(),
            palette: Palette::new(options.color),
            options,
            wifi: wifi::Ui::default(),
            profile: profile::Ui::default(),
            system: system::Ui::default(),
        }
    }
    fn icon<'a>(&self, unicode: &'a str, ascii: &'a str) -> &'a str {
        if self.options.ascii { ascii } else { unicode }
    }
    fn page(&mut self, page: usize) {
        self.page = page % 6;
        self.scroll = 0;
    }
    fn move_row(&mut self, delta: i32) {
        if self.page == 3 {
            self.wifi.move_row(delta);
            return;
        }
        if self.page == 2 {
            let count = self.snapshot.as_ref().map_or(0, |s| s.hardware.len());
            if count == 0 {
                self.table.select(None);
                return;
            }
            let selected = self.table.selected().unwrap_or(0) as i32;
            self.table
                .select(Some((selected + delta).clamp(0, count as i32 - 1) as usize));
        } else {
            self.scroll = (self.scroll as i32 + delta).clamp(0, 200) as u16;
        }
    }
    fn block<'a>(&self, title: impl Into<Line<'a>>, accent: Color) -> Block<'a> {
        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(Style::default().fg(accent).bold())
            .border_style(Style::default().fg(self.palette.muted))
            .style(Style::default().bg(self.palette.panel));
        if self.options.ascii {
            block = block.border_set(symbols::border::Set {
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
    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(self.palette.bg).fg(self.palette.text)),
            area,
        );
        if area.width < 40 || area.height < 12 {
            let notice = format!(
                "AGX SYSTEM CONSOLE\n\nTerminal: {} x {}\nResize to at least 40 x 12.\n\nq / Esc: return to shell",
                area.width, area.height
            );
            frame.render_widget(
                Paragraph::new(notice)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(self.palette.cyan)),
                area,
            );
            self.tabs.clear();
            self.table_area = Rect::default();
            self.wifi.draw_popup(frame, self.palette, self.options);
            return;
        }
        let regions = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
        let fresh = self
            .last_success
            .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
            && self.error.is_none();
        let (status, color) = if self.snapshot.is_none() {
            ("LOADING", self.palette.amber)
        } else if fresh {
            ("LIVE", self.palette.green)
        } else {
            ("STALE", self.palette.red)
        };
        let host = self
            .snapshot
            .as_ref()
            .map_or("connecting", |s| s.hostname.as_str());
        let header = Line::from(vec![
            Span::styled(
                " AGX ",
                Style::default()
                    .fg(self.palette.bg)
                    .bg(self.palette.cyan)
                    .bold(),
            ),
            Span::styled(
                "  SYSTEM CONSOLE ",
                Style::default().fg(self.palette.text).bold(),
            ),
            Span::styled(
                format!(" / {host}   "),
                Style::default().fg(self.palette.muted),
            ),
            Span::styled(
                format!("{} {status}", self.icon("●", "*")),
                Style::default().fg(color).bold(),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(header).block(
                self.block("", self.palette.muted)
                    .borders(Borders::BOTTOM)
                    .style(Style::default().bg(self.palette.bg)),
            ),
            regions[0],
        );
        let tabs = Layout::horizontal([Constraint::Ratio(1, 6); 6]).split(regions[1]);
        self.tabs = tabs.to_vec();
        let names = if area.width < 65 {
            [
                "1 Profile".into(),
                "2 Overview".into(),
                "3 Devices".into(),
                "4 Wi-Fi".into(),
                "5 System".into(),
                "? Help".into(),
            ]
        } else {
            [
                format!("1 {} Profile", self.icon("◉", "@")),
                format!("2 {} Overview", self.icon("◈", ">")),
                format!("3 {} Hardware", self.icon("■", "#")),
                format!("4 {} Network", self.icon("↗", "~")),
                format!("5 {} System", self.icon("⚙", "*")),
                "? Help".into(),
            ]
        };
        for (index, name) in names.into_iter().enumerate() {
            let selected = self.page == index;
            let color = if selected {
                self.palette.cyan
            } else {
                self.palette.muted
            };
            let block = self
                .block(name, color)
                .border_style(Style::default().fg(color));
            frame.render_widget(block, tabs[index]);
        }
        self.table_area = Rect::default();
        match self.page {
            0 => self
                .profile
                .draw(frame, regions[2], self.palette, self.options),
            1 => self.overview(frame, regions[2]),
            2 => self.hardware(frame, regions[2]),
            3 => self
                .wifi
                .draw(frame, regions[2], self.palette, self.options),
            4 => {
                let kernel = self
                    .snapshot
                    .as_ref()
                    .map_or("Loading", |s| s.kernel.as_str());
                self.system
                    .draw(frame, regions[2], self.palette, self.options, kernel);
            }
            _ => self.help(frame, regions[2]),
        }
        let footer_regions =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(14)]).split(regions[3]);
        let footer = if let Some(error) = &self.error {
            format!(" q quit | r retry | DATA ERROR: {error}")
        } else if let Some(notice) = &self.wifi.notice {
            format!(" q quit | {notice}")
        } else if self.snapshot.is_none() {
            " q quit | Collecting live Linux system information...".into()
        } else if area.width < 80 {
            " q quit | ? help | Tab screens | arrows scroll".into()
        } else {
            " q quit  |  Tab switch  |  Up/Down scroll  |  click select  |  r refresh  |  ? help"
                .into()
        };
        frame.render_widget(
            Paragraph::new(footer)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(if self.error.is_some() {
                    self.palette.red
                } else {
                    self.palette.muted
                })),
            footer_regions[0],
        );
        self.wifi
            .draw_reboot(frame, footer_regions[1], self.palette);
        self.wifi.draw_popup(frame, self.palette, self.options);
    }
    fn metric(
        &self,
        frame: &mut Frame,
        area: Rect,
        name: &str,
        value: &str,
        ratio: f64,
        color: Color,
    ) {
        let block = self.block(format!(" {name} "), color);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let regions = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(inner);
        frame.render_widget(
            Paragraph::new(value.to_owned()).style(Style::default().fg(color).bold()),
            regions[0],
        );
        if regions[1].height > 0 && self.options.ascii {
            let filled = (inner.width as f64 * ratio.clamp(0.0, 1.0)).round() as usize;
            let bar = format!(
                "{}{}",
                "#".repeat(filled),
                "-".repeat((inner.width as usize).saturating_sub(filled))
            );
            frame.render_widget(
                Paragraph::new(bar).style(Style::default().fg(color)),
                Rect {
                    height: 1,
                    ..regions[1]
                },
            );
        } else if regions[1].height > 0 {
            frame.render_widget(
                Gauge::default()
                    .ratio(ratio.clamp(0.0, 1.0))
                    .label("")
                    .gauge_style(Style::default().fg(color).bg(self.palette.bg)),
                Rect {
                    height: 1,
                    ..regions[1]
                },
            );
        }
    }
    fn overview(&mut self, frame: &mut Frame, area: Rect) {
        let Some(data) = &self.snapshot else {
            frame.render_widget(
                Paragraph::new("Waiting for the first system sample...")
                    .block(self.block(" Overview ", self.palette.cyan)),
                area,
            );
            return;
        };
        if area.height < 12 || area.width < 65 {
            let used = data.memory.total_bytes - data.memory.available_bytes;
            let mut lines = vec![
                Line::styled(
                    format!(
                        "CPU  {} / {} cores",
                        data.cpu_usage_percent
                            .map(|v| format!("{v:.1}%"))
                            .unwrap_or_else(|| "sampling".into()),
                        data.cpu_count
                    ),
                    Style::default().fg(self.palette.cyan),
                ),
                Line::styled(
                    format!("RAM  {} / {}", bytes(used), bytes(data.memory.total_bytes)),
                    Style::default().fg(self.palette.purple),
                ),
                Line::styled(
                    format!(
                        "ROOT {} free",
                        data.root_storage
                            .as_ref()
                            .map(|v| bytes(v.available_bytes))
                            .unwrap_or_else(|| "unavailable".into())
                    ),
                    Style::default().fg(self.palette.amber),
                ),
                Line::raw(format!("BOARD {}", data.board)),
                Line::raw(format!("KERNEL {}", data.kernel)),
                Line::raw(format!("UPTIME {}", uptime(data.uptime_secs))),
            ];
            for net in data.networks.iter().filter(|n| n.name != "lo") {
                lines.push(Line::styled(
                    format!("{} [{}] {}", net.name, net.state, net.ipv4.join(", ")),
                    Style::default().fg(self.palette.green),
                ));
            }
            for warning in &data.warnings {
                lines.push(Line::styled(
                    format!("! {warning}"),
                    Style::default().fg(self.palette.amber),
                ));
            }
            let block = self.block(" Overview / compact ", self.palette.cyan);
            let inner = block.inner(area);
            let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            self.scroll = self.scroll.min(
                paragraph
                    .line_count(inner.width)
                    .saturating_sub(inner.height as usize) as u16,
            );
            frame.render_widget(paragraph.scroll((self.scroll, 0)).block(block), area);
            return;
        }
        let regions = Layout::vertical([
            Constraint::Length(if area.height >= 15 { 6 } else { 4 }),
            Constraint::Min(3),
        ])
        .split(area);
        let metrics = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(regions[0]);
        let cpu = data
            .cpu_usage_percent
            .map(|v| format!("{v:.1}% / {} cores", data.cpu_count))
            .unwrap_or_else(|| "Sampling CPU...".into());
        self.metric(
            frame,
            metrics[0],
            &format!("{} CPU", self.icon("◆", "*")),
            &cpu,
            data.cpu_usage_percent.unwrap_or(0.0) / 100.0,
            self.palette.cyan,
        );
        let used = data.memory.total_bytes - data.memory.available_bytes;
        self.metric(
            frame,
            metrics[1],
            &format!("{} MEMORY", self.icon("≡", "*")),
            &format!("{} / {}", bytes(used), bytes(data.memory.total_bytes)),
            used as f64 / data.memory.total_bytes as f64,
            self.palette.purple,
        );
        let (disk_text, disk_ratio) = data
            .root_storage
            .as_ref()
            .map(|disk| {
                (
                    format!("{} free", bytes(disk.available_bytes)),
                    1.0 - disk.available_bytes as f64 / disk.total_bytes.max(1) as f64,
                )
            })
            .unwrap_or_else(|| ("Unavailable".into(), 0.0));
        self.metric(
            frame,
            metrics[2],
            &format!("{} ROOT /", self.icon("■", "*")),
            &disk_text,
            disk_ratio,
            self.palette.amber,
        );
        let panels = if area.width >= 90 {
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .split(regions[1])
        } else {
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(regions[1])
        };
        let label = |name: &str, value: String| {
            Line::from(vec![
                Span::styled(
                    format!("{name:<10}"),
                    Style::default().fg(self.palette.muted),
                ),
                Span::raw(value),
            ])
        };
        let mut lines = vec![
            label("HOST", data.hostname.clone()),
            label("BOARD", data.board.clone()),
            label("KERNEL", data.kernel.clone()),
            label("UPTIME", uptime(data.uptime_secs)),
            label(
                "LOAD",
                data.load_average
                    .iter()
                    .map(|v| format!("{v:.2}"))
                    .collect::<Vec<_>>()
                    .join("  "),
            ),
            label("ROOT", "/ (live filesystem usage above)".into()),
        ];
        for warning in &data.warnings {
            lines.push(Line::styled(
                format!("! {warning}"),
                Style::default().fg(self.palette.amber),
            ));
        }
        let block = self.block(
            format!(" {} THIS MACHINE ", self.icon("◈", "*")),
            self.palette.cyan,
        );
        let machine = Paragraph::new(lines).wrap(Wrap { trim: false });
        let mut links = Vec::new();
        if data.networks.is_empty() {
            links.push(Line::raw("No network interfaces detected."));
        }
        for network in data.networks.iter().filter(|net| net.name != "lo") {
            let color = if network.state == "up" {
                self.palette.green
            } else {
                self.palette.amber
            };
            links.push(Line::from(vec![
                Span::styled(
                    format!("{} {}", self.icon("●", "*"), network.name),
                    Style::default().fg(color).bold(),
                ),
                Span::styled(
                    format!("  [{}]", network.state),
                    Style::default().fg(self.palette.muted),
                ),
            ]));
            links.push(Line::raw(if network.ipv4.is_empty() {
                "  No IPv4 address".into()
            } else {
                format!("  {}", network.ipv4.join(", "))
            }));
            links.push(Line::styled(
                format!("  {}", network.mac),
                Style::default().fg(self.palette.muted),
            ));
            links.push(Line::raw(""));
        }
        if links.is_empty() {
            links.push(Line::raw("Only loopback is currently present."));
        }
        let link_block = self.block(
            format!(" {} NETWORK LINKS ", self.icon("↗", ">")),
            self.palette.green,
        );
        let network = Paragraph::new(links).wrap(Wrap { trim: false });
        let machine_inner = block.inner(panels[0]);
        let network_inner = link_block.inner(panels[1]);
        let machine_max = machine
            .line_count(machine_inner.width)
            .saturating_sub(machine_inner.height as usize) as u16;
        let network_max = network
            .line_count(network_inner.width)
            .saturating_sub(network_inner.height as usize) as u16;
        self.scroll = self.scroll.min(machine_max.max(network_max));
        frame.render_widget(
            machine
                .scroll((self.scroll.min(machine_max), 0))
                .block(block),
            panels[0],
        );
        frame.render_widget(
            network
                .scroll((self.scroll.min(network_max), 0))
                .block(link_block),
            panels[1],
        );
    }

    fn hardware(&mut self, frame: &mut Frame, area: Rect) {
        let Some(data) = &self.snapshot else {
            frame.render_widget(
                Paragraph::new("Waiting for hardware inventory...")
                    .block(self.block(" Hardware ", self.palette.cyan)),
                area,
            );
            return;
        };
        let regions = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(area);
        self.table_area = regions[0];
        if !data.hardware.is_empty() {
            self.table.select(Some(
                self.table
                    .selected()
                    .unwrap_or(0)
                    .min(data.hardware.len() - 1),
            ));
        } else {
            self.table.select(None);
        }
        let rows = data.hardware.iter().map(|item| {
            let color = match item.state.as_str() {
                "up" | "detected" | "Live" => self.palette.green,
                "down" => self.palette.amber,
                _ => self.palette.muted,
            };
            Row::new(vec![
                Cell::from(item.kind.clone()).style(Style::default().fg(self.palette.muted)),
                Cell::from(item.name.clone()),
                Cell::from(item.state.clone()).style(Style::default().fg(color)),
                Cell::from(item.detail.clone()),
            ])
        });
        let title = format!(
            " {} HARDWARE INVENTORY / {} items ",
            self.icon("■", "#"),
            data.hardware.len()
        );
        let table = Table::new(
            rows,
            [
                Constraint::Length(9),
                Constraint::Percentage(25),
                Constraint::Length(9),
                Constraint::Min(8),
            ],
        )
        .header(
            Row::new(["TYPE", "DEVICE", "STATE", "DETAIL"])
                .style(Style::default().fg(self.palette.cyan).bold()),
        )
        .row_highlight_style(
            Style::default()
                .bg(self.palette.bg)
                .fg(self.palette.cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(self.icon("› ", "> "))
        .block(self.block(title, self.palette.purple));
        frame.render_stateful_widget(table, regions[0], &mut self.table);
        let detail = self
            .table
            .selected()
            .and_then(|index| data.hardware.get(index))
            .map(|row| {
                format!(
                    "{} / {}: {} [{}]",
                    row.kind, row.name, row.detail, row.state
                )
            })
            .unwrap_or_else(|| "No hardware entries are available.".into());
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .block(self.block(" Selected device ", self.palette.muted)),
            regions[1],
        );
    }
    fn help(&mut self, frame: &mut Frame, area: Rect) {
        let lines = vec![
            Line::styled(
                "HEADLESS BY DESIGN",
                Style::default().fg(self.palette.cyan).bold(),
            ),
            Line::raw("Overview and Hardware are read-only. Network provisions this Pi."),
            Line::raw(""),
            Line::raw(
                "1 / 2 / 3 / 4 / 5 / ?   Profile / Overview / Hardware / Network / System / Help",
            ),
            Line::raw("Tab / Shift-Tab Next / previous screen"),
            Line::raw("Left / Right    Previous / next screen"),
            Line::raw("Up / Down, j/k  Scroll information or select hardware"),
            Line::raw("Home / End      First / last hardware entry"),
            Line::raw("PageUp/PageDown Scroll by a page"),
            Line::raw("Mouse           Click tabs or hardware; wheel to scroll"),
            Line::raw("r               Request a fresh system sample"),
            Line::raw("Network: s scan, Enter connect, h hidden, c country"),
            Line::raw("Network: v saved, f forget, d disconnect, i check Internet"),
            Line::raw("F10             Reboot (confirmation required)"),
            Line::raw("q / Esc / Ctrl-C Return to the shell"),
            Line::raw(""),
            Line::styled(
                "TERMINAL OPTIONS",
                Style::default().fg(self.palette.purple).bold(),
            ),
            Line::raw("agx tui --ascii --no-color --no-mouse"),
            Line::raw("agx tui --color truecolor|256|16|auto|never"),
            Line::raw("NO_COLOR disables color in auto mode."),
            Line::raw("Icons use Unicode; --ascii provides a font-independent fallback."),
            Line::raw(""),
            Line::styled("AUTOMATION", Style::default().fg(self.palette.green).bold()),
            Line::raw("agx status --json emits plain machine-readable data."),
            Line::raw("Exit freely: system services continue after this TUI closes."),
        ];
        let block = self.block(" ? KEYBOARD & MOUSE ", self.palette.cyan);
        let inner = block.inner(area);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        self.scroll = self.scroll.min(
            paragraph
                .line_count(inner.width)
                .saturating_sub(inner.height as usize) as u16,
        );
        frame.render_widget(paragraph.scroll((self.scroll, 0)).block(block), area);
    }
}

fn terminal_closed() -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // descriptor is initialized, count is one, and a zero timeout never blocks.
    if unsafe { libc::poll(&mut descriptor, 1, 0) } < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error);
    }
    Ok(descriptor.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0)
}

pub fn run(options: Options) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "The TUI requires an interactive terminal. Use ssh -t HOST agx tui, or agx status --json for scripts.",
        ));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let mut signals = Signals(Vec::new());
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ] {
        signals
            .0
            .push(signal_hook::flag::register(signal, Arc::clone(&stop))?);
    }
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous_hook(info);
    }));
    let worker = Worker::start()?;
    let wifi_worker = wifi::Worker::start()?;
    let profile_worker = profile::Worker::start()?;
    let _session = Session::enter(options.mouse)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut app = App::new(options);
    let mut dirty = true;
    let mut last_frame = Instant::now();
    let mut rebooting = false;
    while !stop.load(Ordering::Relaxed) && !terminal_closed()? {
        while let Ok(result) = worker.results.try_recv() {
            match result {
                Ok(snapshot) => {
                    app.snapshot = Some(snapshot);
                    app.error = None;
                    app.last_success = Some(Instant::now());
                }
                Err(error) => app.error = Some(error),
            }
            dirty = true;
        }
        while let Ok(update) = wifi_worker.results.try_recv() {
            if update.reboot_accepted {
                rebooting = true;
            }
            app.wifi.update(update);
            dirty = true;
        }
        if rebooting {
            break;
        }
        while let Ok(update) = profile_worker.results.try_recv() {
            app.profile.update(update);
            dirty = true;
        }
        if app.system.poll() {
            dirty = true;
        }
        if dirty || last_frame.elapsed() >= Duration::from_secs(1) {
            terminal.draw(|frame| app.draw(frame))?;
            dirty = false;
            last_frame = Instant::now();
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        if terminal_closed()? {
            break;
        }
        dirty = true;
        let input = event::read()?;
        if matches!(&input, Event::Key(key) if key.kind == KeyEventKind::Release) {
            continue;
        }
        if matches!(&input, Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
        {
            break;
        }
        if app.wifi.has_popup() {
            if let Interaction::Send(command) = app.wifi.popup_event(&input) {
                wifi_worker.send(command, &mut app.wifi);
            }
            continue;
        }
        if app.page == 0 {
            let (consumed, command) = app.profile.event(&input);
            if let Some(command) = command {
                profile_worker.send(command, &mut app.profile);
            }
            if consumed {
                continue;
            }
        }
        if app.page == 4
            && (matches!(&input, Event::Key(key) if key.code == KeyCode::Char('c'))
                || matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.system.button.contains((mouse.column, mouse.row).into())))
        {
            app.system.start();
            continue;
        }
        if matches!(&input, Event::Key(key) if key.code == KeyCode::F(10)) {
            app.wifi.reboot();
            continue;
        }
        if matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.wifi.reboot_area.contains((mouse.column, mouse.row).into()))
        {
            app.wifi.reboot();
            continue;
        }
        if app.page == 3
            && let Event::Key(key) = &input
        {
            match app.wifi.key(*key) {
                Interaction::Consumed => continue,
                Interaction::Send(command) => {
                    wifi_worker.send(command, &mut app.wifi);
                    continue;
                }
                Interaction::Unused => (),
            }
        }
        match input {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('1') => app.page(0),
                    KeyCode::Char('2') => app.page(1),
                    KeyCode::Char('3') => app.page(2),
                    KeyCode::Char('5') => app.page(4),
                    KeyCode::Char('?') | KeyCode::Char('6') => app.page(5),
                    KeyCode::Char('4') => {
                        app.page(3);
                        if let Some(command) = app.wifi.entered() {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    }
                    KeyCode::Tab | KeyCode::Right => {
                        app.page(app.page + 1);
                        if app.page == 3
                            && let Some(command) = app.wifi.entered()
                        {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    }
                    KeyCode::BackTab | KeyCode::Left => {
                        app.page(app.page + 5);
                        if app.page == 3
                            && let Some(command) = app.wifi.entered()
                        {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => app.move_row(1),
                    KeyCode::Up | KeyCode::Char('k') => app.move_row(-1),
                    KeyCode::PageDown => app.move_row(8),
                    KeyCode::PageUp => app.move_row(-8),
                    KeyCode::Home => {
                        app.scroll = 0;
                        if app.page == 3 {
                            app.wifi.home_end(false);
                        } else {
                            app.table.select(Some(0));
                        }
                    }
                    KeyCode::End => {
                        let count = app.snapshot.as_ref().map_or(0, |s| s.hardware.len());
                        if app.page == 3 {
                            app.wifi.home_end(true);
                        } else {
                            app.table.select(count.checked_sub(1));
                        }
                    }
                    KeyCode::Char('r') => {
                        let _ = worker.refresh.try_send(());
                    }
                    _ => (),
                }
            }
            Event::Mouse(mouse) if options.mouse => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(page) = app
                        .tabs
                        .iter()
                        .position(|rect| rect.contains((mouse.column, mouse.row).into()))
                    {
                        app.page(page);
                        if page == 3
                            && let Some(command) = app.wifi.entered()
                        {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    } else if app.page == 2
                        && app.table_area.contains((mouse.column, mouse.row).into())
                        && mouse.row >= app.table_area.y + 2
                        && mouse.row < app.table_area.bottom().saturating_sub(1)
                    {
                        let index =
                            app.table.offset() + (mouse.row - app.table_area.y - 2) as usize;
                        if app
                            .snapshot
                            .as_ref()
                            .is_some_and(|s| index < s.hardware.len())
                        {
                            app.table.select(Some(index));
                        }
                    } else if app.page == 3
                        && let Interaction::Send(command) = app.wifi.mouse(mouse)
                    {
                        wifi_worker.send(command, &mut app.wifi);
                    }
                }
                MouseEventKind::ScrollDown => app.move_row(3),
                MouseEventKind::ScrollUp => app.move_row(-3),
                _ => (),
            },
            Event::Resize(_, _) => (),
            _ => (),
        }
    }
    drop(terminal);
    drop(_session);
    if rebooting {
        use std::io::Write;
        let mut output = io::stdout().lock();
        let _ = writeln!(
            output,
            "\nAGX is shutting down for reboot. Your SSH session will close."
        );
        if let Ok(logo) = std::fs::read_to_string("/usr/share/agx/logo.ascii") {
            for line in logo.lines() {
                let _ = writeln!(output, "{line}");
                let _ = output.flush();
                std::thread::sleep(Duration::from_millis(35));
            }
        }
        let _ = writeln!(output, "\nPlease log back in once AGX is ready.");
        let _ = output.flush();
    }
    Ok(())
}

fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", value as f64 / 1073741824.0)
    } else {
        format!("{:.0} MiB", value as f64 / 1048576.0)
    }
}
fn uptime(seconds: u64) -> String {
    format!(
        "{}d {:02}h {:02}m {:02}s",
        seconds / 86400,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    #[test]
    fn small_and_resized_layouts_keep_help_and_navigation_available() {
        let options = Options {
            ascii: true,
            color: ColorMode::Never,
            mouse: false,
        };
        for (width, height) in [(20, 6), (40, 12), (80, 24), (120, 36)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(options);
            app.snapshot = Some(Sampler::default().collect().unwrap());
            for page in 0..6 {
                app.page(page);
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(text.contains("AGX") || text.contains("Overview"));
                assert!(text.is_ascii(), "ASCII mode emitted Unicode");
                if width >= 40 {
                    assert_eq!(app.tabs.len(), 6);
                    assert!(text.contains("q quit"), "Quit shortcut must remain visible");
                }
            }
        }
    }
}
