//! Shared remote-desktop status panel for Network and Services.
use crate::{Options, Palette};
use agx_core::Snapshot;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Paragraph, Wrap},
};

/// Reserve a status panel when there is room, preserving controls in small terminals.
pub(super) fn panel(
    frame: &mut Frame,
    area: Rect,
    snapshot: Option<&Snapshot>,
    p: Palette,
    options: Options,
) -> Rect {
    if area.height < 6 {
        return area;
    }
    let detailed = area.height >= 20;
    let parts = Layout::vertical([
        Constraint::Length(if detailed { 6 } else { 2 }),
        Constraint::Min(4),
    ])
    .split(area);
    let lines = if let Some(data) = snapshot {
        let status = &data.vnc;
        let state = if let Some(error) = &status.error {
            format!("VNC status unavailable: {error}")
        } else if status.listeners.is_empty() {
            format!(
                "VNC port {}: not listening | {} connection(s)",
                status.port, status.connections
            )
        } else {
            format!(
                "VNC: {} | {} connection(s)",
                status.listeners.join(", "),
                status.connections
            )
        };
        let memory = status.gui_rss_bytes.map_or_else(
            || "GUI: not running or memory unavailable".into(),
            |bytes| {
                format!(
                    "GUI memory: {:.1} MiB RSS (includes VNC)",
                    bytes as f64 / 1048576.0
                )
            },
        );
        let ips = data
            .networks
            .iter()
            .filter(|net| net.name != "lo" && !net.ipv4.is_empty())
            .map(|net| format!("{} {}", net.name, net.ipv4.join(", ")))
            .collect::<Vec<_>>();
        vec![
            Line::raw(state),
            Line::raw(memory),
            Line::raw("VNC runs inside GUI; SSH tunnel required. F8 on HDMI opens GUI."),
            Line::raw(format!(
                "Pi: {}",
                if ips.is_empty() {
                    "no IPv4 addresses".into()
                } else {
                    ips.join(" | ")
                }
            )),
        ]
    } else {
        vec![Line::raw("Collecting VNC socket and GUI memory status...")]
    };
    if detailed {
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(p.block(options, " VNC / REMOTE DESKTOP ", p.cyan)),
            parts[0],
        );
    } else {
        frame.render_widget(
            Paragraph::new(lines.into_iter().take(2).collect::<Vec<_>>()),
            parts[0],
        );
    }
    parts[1]
}
