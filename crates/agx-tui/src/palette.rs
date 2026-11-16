//! Terminal color modes and shared palette.
use crate::ColorMode;
use ratatui::{
    style::{Color, Style},
    symbols,
    text::Line,
    widgets::{Block, Borders},
};

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub(super) bg: Color,
    pub(super) panel: Color,
    pub(super) text: Color,
    pub(super) muted: Color,
    pub(super) cyan: Color,
    pub(super) purple: Color,
    pub(super) green: Color,
    pub(super) amber: Color,
    pub(super) red: Color,
}
impl Palette {
    pub(super) fn new(mode: ColorMode) -> Self {
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

impl Palette {
    pub(super) fn block<'a>(
        self,
        options: crate::Options,
        title: impl Into<Line<'a>>,
        accent: Color,
    ) -> Block<'a> {
        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(Style::default().fg(accent).bold())
            .border_style(Style::default().fg(self.muted))
            .style(Style::default().bg(self.panel));
        if options.ascii {
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
}
