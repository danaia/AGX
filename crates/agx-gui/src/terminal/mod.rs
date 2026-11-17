//! Lazy, native terminal sessions composed into ANGI's existing window manager.
mod input;
mod keys;
mod render;
mod session;
#[cfg(test)]
mod tests;
mod windows;
use crate::{
    App, AppWindow, Terminal, TerminalRow,
    wm::{BORDER, HEADER, Rect},
};
use alacritty_terminal::{
    grid::Scroll,
    index::{Column, Direction, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{TermMode, search::RegexSearch},
};
use session::{Session, Size};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::RefCell, rc::Rc, sync::atomic::Ordering, time::Duration};
#[derive(Default)]
pub(super) struct State {
    snapshot: Option<crate::TerminalView>,
    start_directory: Option<std::path::PathBuf>,
    sessions: Vec<Session>,
    active: usize,
    rows: Rc<VecModel<TerminalRow>>,
    previous: Vec<Vec<render::Run>>,
    clipboard: String,
    pub captured: bool,
    mouse_button: Option<u8>,
    search_origin: Option<Point>,
    attempted: bool,
    last_focus: bool,
    scroll_fraction: f32,
}
fn geometry(rect: Rect, font: f32, search: bool) -> (Rect, Size) {
    let top = if search { 78. } else { 42. };
    let area = Rect::new(
        rect.x + BORDER + 8.,
        rect.y + HEADER + top,
        (rect.w - 2. * BORDER - 16.).max(1.),
        (rect.h - HEADER - BORDER - top - 26.).max(1.),
    );
    (
        area,
        Size {
            cols: (area.w / (font * 0.6)).floor().clamp(2., 400.) as usize,
            lines: (area.h / (font * 1.36).ceil()).floor().clamp(1., 160.) as usize,
        },
    )
}
impl State {
    fn current(&self) -> Option<&Session> {
        self.sessions.get(self.active)
    }
    fn dirty(&self) {
        if let Some(s) = self.current() {
            s.signals.dirty.store(true, Ordering::Release);
        }
    }
    fn tabs(&self, ui: &AppWindow) {
        let labels = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{} · {}{}",
                    i + 1,
                    *s.signals.title.lock().unwrap(),
                    if s.signals.exited.load(Ordering::Acquire) {
                        " (exited)"
                    } else {
                        ""
                    }
                )
                .into()
            })
            .collect::<Vec<_>>();
        let global = ui.global::<Terminal>();
        if global.get_tabs().iter().collect::<Vec<_>>() != labels {
            global.set_tabs(ModelRc::new(VecModel::from(labels)));
        }
        global.set_active_tab(self.active as i32);
    }
}
impl App {
    fn terminal_tick(&mut self, ui: &AppWindow) -> bool {
        if !self.desk.is_open(self.terminal_kind) {
            if !self.terminal.sessions.is_empty() || self.terminal.attempted {
                self.terminal = State::default();
                ui.global::<Terminal>().set_rows(ModelRc::default());
            }
            return false;
        }
        let placed = self
            .view
            .scene
            .borrow()
            .windows
            .iter()
            .find(|w| w.kind == self.terminal_kind && !w.collapsed)
            .copied();
        let Some(placed) = placed else {
            if self.terminal.last_focus {
                if let Some(s) = self.terminal.current() {
                    if s.term.lock().mode().contains(TermMode::FOCUS_IN_OUT) {
                        s.write(b"\x1b[O".to_vec());
                    }
                }
                self.terminal.last_focus = false;
            }
            self.terminal.captured = false;
            self.terminal.mouse_button = None;
            return false;
        };
        if self.terminal.sessions.is_empty() && !self.terminal.attempted {
            self.terminal_action(ui, 0, 0);
        }
        let global = ui.global::<Terminal>();
        let (_, size) = geometry(
            placed.rect,
            global.get_font_size(),
            global.get_search_visible(),
        );
        let state = &mut self.terminal;
        let Some(session) = state.sessions.get_mut(state.active) else {
            return true;
        };
        session.resize(size);
        let focused = self.desk.focused() == Some(self.terminal_kind) && !ui.get_menu_open();
        if focused != state.last_focus {
            if session.term.lock().mode().contains(TermMode::FOCUS_IN_OUT) {
                session.write(if focused { b"\x1b[I" } else { b"\x1b[O" }.to_vec());
            }
            state.last_focus = focused;
            session.signals.dirty.store(true, Ordering::Release);
        }
        global.set_focused(focused);
        if !session.signals.dirty.swap(false, Ordering::AcqRel) {
            return false;
        }
        let Some(term) = session.term.try_lock_unfair() else {
            session.signals.dirty.store(true, Ordering::Release);
            return false;
        };
        let rows = render::rows(&term);
        if session.starting
            && (session.signals.exited.load(Ordering::Acquire)
                || rows.iter().flatten().any(|run| !run.text.trim().is_empty()))
        {
            session.starting = false;
        }
        global.set_busy(session.starting);
        // Each window snapshot publishes a new row model when its content changes.
        // Nested Slint models otherwise retain glyph bindings from an earlier snapshot.
        state.rows = Rc::new(VecModel::from(
            rows.iter().map(|r| render::model(r)).collect::<Vec<_>>(),
        ));
        state.previous = rows;
        let content = term.renderable_content();
        let y = content.cursor.point.line.0 + content.display_offset as i32;
        global.set_cursor_col(content.cursor.point.column.0 as i32);
        global.set_cursor_row(y);
        global.set_cursor_visible(
            content.mode.contains(TermMode::SHOW_CURSOR) && y >= 0 && y < (size.lines as i32),
        );
        global.set_title(format!("Terminal · {}", *session.signals.title.lock().unwrap()).into());
        global.set_status(
            format!(
                "{} × {}  ·  {}{}  ·  Ctrl+Shift+T new tab · Ctrl+Shift+F search",
                size.cols,
                size.lines,
                if session.signals.exited.load(Ordering::Acquire) {
                    "Exited"
                } else {
                    "Terminal"
                },
                if content.display_offset > 0 {
                    format!(" · ↑ {} lines", content.display_offset)
                } else {
                    String::new()
                }
            )
            .into(),
        );
        drop(term);
        global.set_rows(ModelRc::from(state.rows.clone()));
        state.tabs(ui);
        true
    }
    pub(crate) fn terminal_open_opencode(&mut self, ui: &AppWindow) {
        if !std::path::Path::new("/usr/bin/opencode").is_file() {
            self.toast(ui, "OpenCode is not installed.");
            return;
        }
        self.terminal_select(ui, crate::wm::Kind::Terminal);
        self.desk.open(self.terminal_kind);
        self.refresh(ui);
        self.terminal_action(ui, 10, 0);
    }
    pub(crate) fn terminal_open_agent(&mut self, ui: &AppWindow, prompt: String, project: Option<String>) {
        if prompt.trim().is_empty() {
            return;
        }
        self.terminal_select(ui, crate::wm::Kind::Terminal);
        self.desk.open(self.terminal_kind);
        self.refresh(ui);
        let global = ui.global::<Terminal>();
        let rect = self
            .view
            .scene
            .borrow()
            .windows
            .iter()
            .find(|w| w.kind == self.terminal_kind)
            .map(|w| w.rect)
            .unwrap_or(Rect::new(0., 0., 800., 500.));
        let (_, size) = geometry(rect, global.get_font_size(), global.get_search_visible());
        self.terminal.attempted = true;
        match Session::agent(size, prompt, project) {
            Ok(session) => {
                self.terminal.sessions.push(session);
                self.terminal.active = self.terminal.sessions.len() - 1;
                global.set_error("".into());
                global.set_busy(true);
            }
            Err(error) => global.set_error(format!("Unable to start Agent Angie: {error}").into()),
        }
    }
    fn terminal_action(&mut self, ui: &AppWindow, action: i32, index: i32) {
        let global = ui.global::<Terminal>();
        if matches!(action, 0..=2 | 10) && self.terminal.last_focus {
            if let Some(s) = self.terminal.current() {
                if s.term.lock().mode().contains(TermMode::FOCUS_IN_OUT) {
                    s.write(b"\x1b[O".to_vec());
                }
            }
            self.terminal.last_focus = false;
        }
        match action {
            0 | 10 => {
                self.terminal.attempted = true;
                let rect = self
                    .view
                    .scene
                    .borrow()
                    .windows
                    .iter()
                    .find(|w| w.kind == self.terminal_kind)
                    .map(|w| w.rect)
                    .unwrap_or(Rect::new(0., 0., 800., 500.));
                let (_, size) = geometry(rect, global.get_font_size(), global.get_search_visible());
                match if action == 10 {
                    Session::opencode(size)
                } else {
                    match self
                        .terminal
                        .current()
                        .and_then(Session::working_directory)
                        .or_else(|| self.terminal.start_directory.clone())
                    {
                        Some(directory) => Session::in_directory(size, Some(directory)),
                        None => Session::new(size),
                    }
                } {
                    Ok(s) => {
                        self.terminal.sessions.push(s);
                        self.terminal.active = self.terminal.sessions.len() - 1;
                        global.set_error("".into());
                        global.set_busy(action == 10);
                    }
                    Err(e) => {
                        global.set_error(format!("Unable to start shell: {e}").into());
                        return;
                    }
                }
            }
            1 if index >= 0 && (index as usize) < self.terminal.sessions.len() => {
                self.terminal.active = index as usize
            }
            2 if index >= 0 && (index as usize) < self.terminal.sessions.len() => {
                self.terminal.sessions.remove(index as usize);
                if self.terminal.sessions.is_empty() {
                    self.desk.close(self.terminal_kind);
                    self.terminal = State::default();
                    global.set_rows(ModelRc::default());
                    self.refresh(ui);
                    return;
                }
                if (index as usize) < self.terminal.active {
                    self.terminal.active -= 1;
                }
                self.terminal.active = self.terminal.active.min(self.terminal.sessions.len() - 1);
            }
            3 => {
                if let Some(s) = self.terminal.sessions.get(self.terminal.active) {
                    if let Some(text) = s.term.lock().selection_to_string() {
                        self.terminal.clipboard = text.clone();
                        global.set_clipboard(text.into());
                        global.set_copy_serial(global.get_copy_serial() + 1);
                    }
                }
                return;
            }
            4 => {
                global.set_paste_serial(global.get_paste_serial() + 1);
                return;
            }
            5 => {
                global.set_search_visible(!global.get_search_visible());
                if !global.get_search_visible() {
                    ui.invoke_focus_desktop();
                }
                self.terminal.search_origin = None;
            }
            6 => {
                if let Some(s) = self.terminal.sessions.get(self.terminal.active) {
                    s.term.lock().scroll_display(Scroll::Bottom);
                }
            }
            7 => global.set_font_size((global.get_font_size() + index as f32).clamp(10., 26.)),
            8 | 9 => {
                self.terminal_search(ui, global.get_query().as_str(), action == 8);
                return;
            }
            _ => return,
        }
        self.terminal.previous.clear();
        self.terminal.captured = false;
        self.terminal.search_origin = None;
        self.terminal.dirty();
        self.terminal.tabs(ui);
    }
    fn terminal_paste(&mut self, text: &str) {
        let text = if text.is_empty() {
            &self.terminal.clipboard
        } else {
            text
        };
        if let Some(s) = self.terminal.sessions.get(self.terminal.active) {
            let bracketed = s.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
            s.write(keys::paste(text, bracketed));
        }
    }
    fn terminal_search(&mut self, ui: &AppWindow, query: &str, backwards: bool) {
        let Some(s) = self.terminal.sessions.get(self.terminal.active) else {
            return;
        };
        let mut regex = match RegexSearch::new(query) {
            Ok(regex) => regex,
            Err(_) => {
                ui.global::<Terminal>()
                    .set_status("Invalid search expression".into());
                return;
            }
        };
        let mut term = s.term.lock();
        let origin = self
            .terminal
            .search_origin
            .unwrap_or_else(|| Point::new(Line(-(term.grid().display_offset() as i32)), Column(0)));
        if let Some(found) = term.search_next(
            &mut regex,
            origin,
            if backwards {
                Direction::Left
            } else {
                Direction::Right
            },
            if backwards { Side::Left } else { Side::Right },
            None,
        ) {
            let (start, end) = (*found.start(), *found.end());
            let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
            selection.update(end, Side::Right);
            term.selection = Some(selection);
            term.scroll_to_point(start);
            self.terminal.search_origin = Some(if backwards { start } else { end });
            s.signals.dirty.store(true, Ordering::Release);
        } else {
            ui.global::<Terminal>()
                .set_status("No matching text in scrollback".into());
        }
    }
}
pub(super) fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Timer {
    let (weak, state) = (ui.as_weak(), app.clone());
    ui.global::<Terminal>().on_action(move |action, index| {
        if let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) {
            app.terminal_action(&ui, action, index);
        }
    });
    let state = app.clone();
    ui.global::<Terminal>().on_paste(move |text| {
        if let Ok(mut app) = state.try_borrow_mut() {
            app.terminal_paste(&text);
        }
    });
    let (weak, state) = (ui.as_weak(), app.clone());
    ui.global::<Terminal>().on_search(move |query, back| {
        if let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) {
            app.terminal_search(&ui, &query, back);
        }
    });
    let timer = Timer::default();
    let (weak, state) = (ui.as_weak(), app.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) {
            app.terminal_windows_tick(&ui);
        }
    });
    timer
}
