//! Terminal launch screen, reusing the caller's terminal rather than a nested renderer.
use super::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Paragraph, Wrap},
};
impl App {
    pub(super) fn shell(&mut self, frame: &mut Frame, area: Rect) {
        let block = self.block(" 8 TERMINAL ", self.palette.cyan);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let regions = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(inner);
        let opencode = crate::shell::available_opencode();
        let tmux = crate::shell::available_tmux();
        let lines = vec![
            Line::styled(
                "YOUR SHELL, INSIDE ANGI",
                Style::default().fg(self.palette.cyan).bold(),
            ),
            Line::raw(""),
            Line::raw("Enter / F4  Open Bash (or your available system shell)."),
            Line::raw("Type exit or press Ctrl+D to return to this screen."),
            Line::raw(""),
            Line::raw("o  OpenCode AI coding agent; /connect adds your provider."),
            Line::raw("t  Open or reattach your persistent tmux session."),
            Line::raw("tmux: Ctrl+B then D detaches and returns here."),
            Line::raw("tmux: Ctrl+B then C opens a tab; % / \" splits panes."),
            Line::raw("tmux: Ctrl+B then [ searches and scrolls history."),
            Line::raw(""),
            Line::raw("Bash, tmux, Vim, less and OpenSSH use the installed CLI runtime."),
            Line::raw("Your terminal provides fonts, colors, selection and copy/paste."),
            Line::raw(if tmux {
                "tmux is installed. Detached sessions keep running."
            } else {
                "tmux is not installed; the regular shell is available."
            }),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            regions[0],
        );
        self.shell_button = regions[1];
        self.opencode_button = regions[3];
        self.tmux_button = regions[2];
        frame.render_widget(
            Paragraph::new("Open shell  Enter / F4")
                .centered()
                .block(self.block("", self.palette.cyan)),
            self.shell_button,
        );
        frame.render_widget(
            Paragraph::new("Persistent session  t")
                .centered()
                .block(self.block(
                    "",
                    if tmux {
                        self.palette.green
                    } else {
                        self.palette.muted
                    },
                )),
            self.tmux_button,
        );
        frame.render_widget(
            Paragraph::new("OpenCode  o").centered().block(self.block(
                "",
                if opencode {
                    self.palette.cyan
                } else {
                    self.palette.muted
                },
            )),
            self.opencode_button,
        );
    }
}
