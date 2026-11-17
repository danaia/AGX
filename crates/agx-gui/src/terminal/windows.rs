//! Independent terminal window contexts and UI snapshots.
use super::State;
use crate::{App, AppWindow, Terminal, TerminalView, wm::Kind};
use slint::{ComponentHandle, Model, ModelRc};
impl App {
    pub(crate) fn terminal_select(&mut self, ui: &AppWindow, kind: Kind) {
        if !kind.is_terminal() || kind == self.terminal_kind {
            return;
        }
        self.terminal.snapshot = Some(snapshot(ui));
        let previous = std::mem::take(&mut self.terminal);
        self.terminal_windows.insert(self.terminal_kind, previous);
        self.terminal = self.terminal_windows.remove(&kind).unwrap_or_default();
        self.terminal_kind = kind;
        restore(
            ui,
            self.terminal
                .snapshot
                .clone()
                .unwrap_or_else(|| TerminalView {
                    font_size: 14.,
                    title: "Terminal".into(),
                    status: "Opening shell…".into(),
                    ..Default::default()
                }),
        );
    }
    pub(crate) fn terminal_new_window(&mut self, ui: &AppWindow) {
        let directory = if let Some(source) = self.desk.focused().filter(|kind| kind.is_terminal())
        {
            self.terminal_select(ui, source);
            self.terminal
                .current()
                .and_then(super::session::Session::working_directory)
        } else {
            None
        };
        let kind = Kind::TerminalWindow(self.next_terminal);
        self.next_terminal += 1;
        self.desk.open(kind);
        self.terminal_select(ui, kind);
        self.terminal.start_directory = directory;
        self.refresh(ui);
    }
    pub(super) fn terminal_windows_tick(&mut self, ui: &AppWindow) {
        let mut kinds: Vec<_> = self
            .view
            .scene
            .borrow()
            .windows
            .iter()
            .filter(|w| w.kind.is_terminal())
            .map(|w| w.kind)
            .collect();
        let mut closed: Vec<_> = self
            .terminal_windows
            .keys()
            .copied()
            .filter(|k| !self.desk.is_open(*k))
            .collect();
        for kind in closed.drain(..) {
            self.terminal_windows.remove(&kind);
            let index = if let Kind::TerminalWindow(id) = kind {
                id as usize + 1
            } else {
                0
            };
            if index < self.terminal_views.row_count() {
                self.terminal_views
                    .set_row_data(index, TerminalView::default());
            }
        }

        for kind in self
            .terminal_windows
            .keys()
            .copied()
            .chain(std::iter::once(self.terminal_kind))
        {
            if self.desk.is_open(kind) && !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        for kind in kinds {
            self.terminal_select(ui, kind);
            let changed = self.terminal_tick(ui);
            let index = if let Kind::TerminalWindow(id) = kind {
                id as usize + 1
            } else {
                0
            };
            while self.terminal_views.row_count() <= index {
                self.terminal_views.push(TerminalView::default());
            }
            if changed {
                self.terminal_views.set_row_data(index, snapshot(ui));
            }
        }
        ui.global::<Terminal>()
            .set_windows(ModelRc::from(self.terminal_views.clone()));
        if let Some(kind) = self.desk.focused().filter(|k| k.is_terminal()) {
            self.terminal_select(ui, kind);
        }
        if !self.desk.is_open(self.terminal_kind) {
            self.terminal = State::default();
        }
    }
}
fn snapshot(ui: &AppWindow) -> TerminalView {
    let global = ui.global::<Terminal>();
    TerminalView {
        rows: global.get_rows(),
        tabs: global.get_tabs(),
        active_tab: global.get_active_tab(),
        title: global.get_title(),
        status: global.get_status(),
        error: global.get_error(),
        busy: global.get_busy(),
        clipboard: global.get_clipboard(),
        font_size: global.get_font_size(),
        cursor_col: global.get_cursor_col(),
        cursor_row: global.get_cursor_row(),
        cursor_visible: global.get_cursor_visible(),
        focused: global.get_focused(),
        search_visible: global.get_search_visible(),
        query: global.get_query(),
        copy_serial: global.get_copy_serial(),
        paste_serial: global.get_paste_serial(),
        search_serial: global.get_search_serial(),
    }
}
fn restore(ui: &AppWindow, view: TerminalView) {
    let global = ui.global::<Terminal>();
    global.set_rows(view.rows);
    global.set_tabs(view.tabs);
    global.set_active_tab(view.active_tab);
    global.set_title(view.title);
    global.set_status(view.status);
    global.set_error(view.error);
    global.set_busy(view.busy);
    global.set_clipboard(view.clipboard);
    global.set_font_size(view.font_size);
    global.set_cursor_col(view.cursor_col);
    global.set_cursor_row(view.cursor_row);
    global.set_cursor_visible(view.cursor_visible);
    global.set_focused(view.focused);
    global.set_search_visible(view.search_visible);
    global.set_query(view.query);
    global.set_copy_serial(view.copy_serial);
    global.set_paste_serial(view.paste_serial);
    global.set_search_serial(view.search_serial);
}
