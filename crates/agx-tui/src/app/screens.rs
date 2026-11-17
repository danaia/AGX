//! Read-only overview, hardware inventory and help screen rendering.
use super::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Gauge, Paragraph, Row, Table, Wrap},
};

impl App {
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
    pub(super) fn overview(&mut self, frame: &mut Frame, area: Rect) {
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

    pub(super) fn hardware(&mut self, frame: &mut Frame, area: Rect) {
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
    pub(super) fn help(&mut self, frame: &mut Frame, area: Rect) {
        let lines = vec![
            Line::styled(
                "HEADLESS BY DESIGN",
                Style::default().fg(self.palette.cyan).bold(),
            ),
            Line::raw("Overview and Hardware are read-only. Network provisions this Pi."),
            Line::raw(""),
            Line::raw(
                "1-8 / ?   Profile / Overview / Hardware / Network / System / Services / Logs / Terminal / Help",
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
            Line::raw("Services: s start, x stop, t restart (confirmation required)"),
            Line::raw("Logs: f service filter, / search, End follow live events"),
            Line::raw("F4              Open shell; exit returns to the TUI"),
            Line::raw("Terminal: t opens tmux; Ctrl+B then D returns here"),
            Line::raw("F8              Back to GUI (HDMI console)"),
            Line::raw("F9 / F10        Power off / reboot (confirmation required)"),
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
