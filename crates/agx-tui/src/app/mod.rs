//! Console state and screen navigation. Rendering lives in child modules.
mod render;
mod screens;
mod shell;
use crate::terminal::gui_available;
use crate::{Options, Palette, profile, services, system, wifi};
use agx_core::Snapshot;
use ratatui::{
    layout::Rect,
    style::Color,
    text::Line,
    widgets::{Block, TableState},
};
use std::time::Instant;

pub(crate) struct App {
    pub(super) gui_available: bool,
    pub(super) gui_footer: Rect,
    pub(super) shell_footer: Rect,
    pub(super) shell_button: Rect,
    pub(super) tmux_button: Rect,
    pub(super) opencode_button: Rect,
    pub(super) page: usize,
    pub(super) snapshot: Option<Snapshot>,
    pub(super) error: Option<String>,
    pub(super) last_success: Option<Instant>,
    pub(super) scroll: u16,
    pub(super) table: TableState,
    pub(super) tabs: Vec<Rect>,
    pub(super) table_area: Rect,
    pub(super) options: Options,
    pub(super) palette: Palette,
    pub(super) wifi: wifi::Ui,
    pub(super) profile: profile::Ui,
    pub(super) system: system::Ui,
    pub(super) services: services::Ui,
}
impl App {
    pub(super) fn block<'a>(&self, title: impl Into<Line<'a>>, accent: Color) -> Block<'a> {
        self.palette.block(self.options, title, accent)
    }

    pub(super) fn new(options: Options) -> Self {
        let gui_available = gui_available();
        let mut profile = profile::Ui::default();
        profile.gui_available = gui_available;
        Self {
            gui_available,
            gui_footer: Rect::default(),
            shell_footer: Rect::default(),
            shell_button: Rect::default(),
            tmux_button: Rect::default(),
            opencode_button: Rect::default(),
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
            profile,
            system: system::Ui::default(),
            services: services::Ui::default(),
        }
    }
    fn icon<'a>(&self, unicode: &'a str, ascii: &'a str) -> &'a str {
        if self.options.ascii { ascii } else { unicode }
    }
    pub(super) fn page(&mut self, page: usize) {
        self.page = page % 9;
        self.scroll = 0;
    }
    pub(super) fn move_row(&mut self, delta: i32) {
        if self.page == 5 || self.page == 6 {
            self.services.move_row(delta, self.page == 6);
            return;
        }
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
}
