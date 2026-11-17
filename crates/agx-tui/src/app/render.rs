//! Application chrome, screen composition and shared panel borders.
use super::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::time::Duration;

impl App {
    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(self.palette.bg).fg(self.palette.text)),
            area,
        );
        if area.width < 40 || area.height < 12 {
            let notice = format!(
                "ANGI SYSTEM CONSOLE\n\nTerminal: {} x {}\nResize to at least 40 x 12.\n\nq / Esc: return to shell",
                area.width, area.height
            );
            frame.render_widget(
                Paragraph::new(notice)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(self.palette.cyan)),
                area,
            );
            self.tabs.clear();
            self.shell_footer = Rect::default();
            self.gui_footer = Rect::default();
            self.shell_button = Rect::default();
            self.tmux_button = Rect::default();
            self.opencode_button = Rect::default();
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
                " ANGI ",
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
        let tabs = Layout::horizontal([Constraint::Ratio(1, 9); 9]).split(regions[1]);
        self.tabs = tabs.to_vec();
        let names = if area.width < 65 {
            [
                "1 Profile".into(),
                "2 Overview".into(),
                "3 Devices".into(),
                "4 Wi-Fi".into(),
                "5 System".into(),
                "6 Services".into(),
                "7 Logs".into(),
                "8 Terminal".into(),
                "? Help".into(),
            ]
        } else {
            [
                format!("1 {} Profile", self.icon("◉", "@")),
                format!("2 {} Overview", self.icon("◈", ">")),
                format!("3 {} Hardware", self.icon("■", "#")),
                format!("4 {} Network", self.icon("↗", "~")),
                format!("5 {} System", self.icon("⚙", "*")),
                "6 Services".into(),
                "7 Logs".into(),
                "8 Terminal".into(),
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
        self.shell_button = Rect::default();
        self.tmux_button = Rect::default();
        self.opencode_button = Rect::default();
        match self.page {
            0 => self
                .profile
                .draw(frame, regions[2], self.palette, self.options),
            1 => self.overview(frame, regions[2]),
            2 => self.hardware(frame, regions[2]),
            3 => {
                let area = crate::remote_access::panel(
                    frame,
                    regions[2],
                    self.snapshot.as_ref(),
                    self.palette,
                    self.options,
                );
                self.wifi.draw(frame, area, self.palette, self.options);
            }
            4 => {
                let kernel = self
                    .snapshot
                    .as_ref()
                    .map_or("Loading", |s| s.kernel.as_str());
                self.system
                    .draw(frame, regions[2], self.palette, self.options, kernel);
            }
            5 | 6 => {
                let area = if self.page == 5 {
                    crate::remote_access::panel(
                        frame,
                        regions[2],
                        self.snapshot.as_ref(),
                        self.palette,
                        self.options,
                    )
                } else {
                    regions[2]
                };
                self.services
                    .draw(frame, area, self.palette, self.options, self.page == 6);
            }
            7 => self.shell(frame, regions[2]),
            _ => self.help(frame, regions[2]),
        }
        let action_width = if area.width < 70 { 6 } else { 14 };
        let footer_regions = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(action_width),
            Constraint::Length(action_width),
            Constraint::Length(action_width),
            Constraint::Length(action_width),
        ])
        .split(regions[3]);
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
        self.shell_footer = footer_regions[1];
        frame.render_widget(
            Paragraph::new(if area.width < 70 {
                "F4 sh"
            } else {
                "[Shell F4]"
            })
            .centered()
            .style(Style::default().fg(self.palette.cyan)),
            self.shell_footer,
        );
        self.gui_footer = footer_regions[2];
        frame.render_widget(
            Paragraph::new(if area.width < 70 {
                "F8 GUI"
            } else {
                "[GUI F8]"
            })
            .centered()
            .style(Style::default().fg(if self.gui_available {
                self.palette.cyan
            } else {
                self.palette.muted
            })),
            self.gui_footer,
        );
        self.wifi
            .draw_shutdown(frame, footer_regions[3], self.palette);
        self.wifi
            .draw_reboot(frame, footer_regions[4], self.palette);
        self.wifi.draw_popup(frame, self.palette, self.options);
    }
}
