//! Terminal input, selection, mouse reporting and chrome hit testing.
use super::{geometry, keys};
use crate::{
    App, AppWindow, Terminal,
    input::Drag,
    keymap::Mods,
    wm::{BORDER, HEADER, Hit, Kind},
};
use alacritty_terminal::{
    grid::Scroll,
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::TermMode,
};
use slint::ComponentHandle;
impl App {
    pub(crate) fn terminal_shortcut(
        &mut self,
        ui: &AppWindow,
        name: &str,
        mods: Mods,
    ) -> Option<bool> {
        if self.desk.focused() != Some(Kind::Terminal) || self.modk.held(mods) || ui.get_menu_open()
        {
            return None;
        }
        if mods.alt && name == "tab" {
            return None;
        }
        let action = if mods.ctrl && mods.shift {
            match name {
                "t" => Some((0, 0)),
                "w" => Some((2, self.terminal.active as i32)),
                "c" => Some((3, 0)),
                "v" => Some((4, 0)),
                "f" => Some((5, 0)),
                "+" | "=" => Some((7, 1)),
                "-" => Some((7, -1)),
                _ => None,
            }
        } else {
            None
        };
        if let Some((action, index)) = action {
            self.terminal_action(ui, action, index);
            return Some(true);
        }
        if mods.ctrl && matches!(name, "pageup" | "pagedown") && !self.terminal.sessions.is_empty()
        {
            let n = self.terminal.sessions.len();
            let index = (self.terminal.active + if name == "pageup" { n - 1 } else { 1 }) % n;
            self.terminal_action(ui, 1, index as i32);
            return Some(true);
        }
        if name == "escape" && ui.global::<Terminal>().get_search_visible() {
            self.terminal_action(ui, 5, 0);
            return Some(true);
        }
        Some(false)
    }
    pub(crate) fn terminal_key(
        &mut self,
        ui: &AppWindow,
        text: &str,
        mods: Mods,
        down: bool,
    ) -> bool {
        if self.desk.focused() != Some(Kind::Terminal)
            || self.modk.held(mods)
            || ui.get_menu_open()
            || ui.global::<Terminal>().get_search_visible()
        {
            return false;
        }
        if !down {
            return true;
        }
        let Some(s) = self.terminal.sessions.get(self.terminal.active) else {
            return true;
        };
        let mode = *s.term.lock().mode();
        if mods.shift
            && (text
                .chars()
                .eq(std::iter::once(char::from(slint::platform::Key::PageUp)))
                || text
                    .chars()
                    .eq(std::iter::once(char::from(slint::platform::Key::PageDown))))
        {
            s.term.lock().scroll_display(
                if text
                    .chars()
                    .eq(std::iter::once(char::from(slint::platform::Key::PageUp)))
                {
                    Scroll::PageUp
                } else {
                    Scroll::PageDown
                },
            );
            self.terminal.dirty();
            return true;
        }
        if let Some(bytes) = keys::encode(text, mods, mode) {
            {
                let mut term = s.term.lock();
                term.scroll_display(Scroll::Bottom);
                term.selection = None;
            }
            s.write(bytes);
            self.terminal.dirty();
            true
        } else {
            false
        }
    }
    pub(crate) fn terminal_pointer(
        &mut self,
        ui: &AppWindow,
        kind: i32,
        button: i32,
        x: f32,
        y: f32,
        mods: Mods,
    ) -> bool {
        if !matches!(self.drag, Drag::None) || self.modk.held(mods) {
            return false;
        }
        let scene = self.view.scene.borrow();
        let body = matches!(scene.hit(x, y), Hit::Body(Kind::Terminal));
        if !body && !self.terminal.captured {
            return false;
        }
        let Some(placed) = scene
            .windows
            .iter()
            .find(|w| w.kind == Kind::Terminal && !w.collapsed)
        else {
            return false;
        };
        let rect = placed.rect;
        drop(scene);
        self.last = (x, y);
        if kind == 0 {
            self.desk.focus(Kind::Terminal);
            self.refresh(ui);
        }
        let global = ui.global::<Terminal>();
        let font = global.get_font_size();
        let (area, size) = geometry(rect, font, global.get_search_visible());
        let local_x = x - rect.x - BORDER;
        let local_y = y - rect.y - HEADER;
        let width = rect.w - 2. * BORDER;
        if local_y < 34. && !self.terminal.captured {
            ui.set_cursor(6);
            if kind == 0 && button == 0 {
                if local_x >= width - 96. {
                    self.terminal_action(
                        ui,
                        if local_x < width - 64. {
                            0
                        } else if local_x < width - 32. {
                            5
                        } else {
                            6
                        },
                        0,
                    );
                } else {
                    let n = self.terminal.sessions.len().max(1);
                    let tab = ((width - 108.) / n as f32).clamp(1., 180.);
                    let index = ((local_x - 4.) / tab).floor() as i32;
                    self.terminal_action(
                        ui,
                        if tab > 50. && (local_x - 4.) % tab > tab - 20. {
                            2
                        } else {
                            1
                        },
                        index,
                    );
                }
            }
            return true;
        }
        if y < area.y && !self.terminal.captured {
            if kind == 0 && button == 0 {
                if local_x > width - 96. {
                    self.terminal_action(
                        ui,
                        if local_x < width - 64. {
                            8
                        } else if local_x < width - 32. {
                            9
                        } else {
                            5
                        },
                        0,
                    );
                } else {
                    global.set_search_serial(global.get_search_serial() + 1);
                }
            }
            return true;
        }
        ui.set_cursor(7);
        let col = ((x - area.x) / (font * 0.6))
            .floor()
            .clamp(0., (size.cols - 1) as f32) as usize;
        let row = ((y - area.y) / (font * 1.36).ceil())
            .floor()
            .clamp(0., (size.lines - 1) as f32) as i32;
        if kind == 0 && global.get_search_visible() {
            global.set_search_visible(false);
            ui.invoke_focus_desktop();
        }
        let Some(s) = self.terminal.sessions.get(self.terminal.active) else {
            return true;
        };
        let mode = *s.term.lock().mode();
        if mode.intersects(TermMode::MOUSE_MODE)
            && !mods.shift
            && !(self.terminal.captured && self.terminal.mouse_button.is_none())
        {
            let released = kind == 1 || kind == 3;
            let code = match button {
                0 => 0,
                1 => 2,
                2 => 1,
                _ => self.terminal.mouse_button.unwrap_or(3),
            };
            if kind == 0 {
                self.terminal.mouse_button = Some(code);
                self.terminal.captured = true;
            }
            if kind == 2
                && !mode.contains(TermMode::MOUSE_MOTION)
                && !(mode.contains(TermMode::MOUSE_DRAG) && self.terminal.captured)
            {
                return true;
            }
            let code = code
                + if kind == 2 { 32 } else { 0 }
                + if mods.ctrl { 16 } else { 0 }
                + if mods.alt { 8 } else { 0 };
            self.terminal
                .mouse_report(code, col, row as usize, released);
            if released {
                self.terminal.mouse_button = None;
                self.terminal.captured = false;
            }
            return true;
        }
        if kind == 0 && button != 0 {
            self.terminal_action(ui, 4, 0);
            return true;
        }
        if kind == 0 || (kind == 2 && self.terminal.captured) {
            let mut term = s.term.lock();
            let point = Point::new(Line(row - term.grid().display_offset() as i32), Column(col));
            let side = if (x - area.x) % (font * 0.6) < font * 0.3 {
                Side::Left
            } else {
                Side::Right
            };
            if kind == 0 {
                term.selection = Some(Selection::new(
                    if mods.alt {
                        SelectionType::Block
                    } else {
                        SelectionType::Simple
                    },
                    point,
                    side,
                ));
            } else if let Some(selection) = term.selection.as_mut() {
                selection.update(point, side);
            }
            drop(term);
            self.terminal.captured = true;
        }
        if kind == 1 || kind == 3 {
            self.terminal.captured = false;
        }
        self.terminal.dirty();
        true
    }
    pub(crate) fn terminal_scroll(&mut self, ui: &AppWindow, delta: f32, mods: Mods) -> bool {
        if self.modk.held(mods)
            || !matches!(
                self.view.scene.borrow().hit(self.last.0, self.last.1),
                Hit::Body(Kind::Terminal)
            )
        {
            return false;
        }
        if mods.ctrl {
            if delta != 0. {
                self.terminal_action(ui, 7, if delta > 0. { 1 } else { -1 });
            }
            return true;
        }
        self.terminal.scroll_fraction += delta / 19.;
        let lines = self.terminal.scroll_fraction.trunc() as i32;
        self.terminal.scroll_fraction -= lines as f32;
        let Some(s) = self.terminal.sessions.get(self.terminal.active) else {
            return true;
        };
        let mode = *s.term.lock().mode();
        if mode.intersects(TermMode::MOUSE_MODE) && !mods.shift {
            let scene = self.view.scene.borrow();
            let Some(p) = scene.windows.iter().find(|p| p.kind == Kind::Terminal) else {
                return true;
            };
            let (area, size) = geometry(
                p.rect,
                ui.global::<Terminal>().get_font_size(),
                ui.global::<Terminal>().get_search_visible(),
            );
            let col = ((self.last.0 - area.x) / (ui.global::<Terminal>().get_font_size() * 0.6))
                .max(0.) as usize;
            let row = ((self.last.1 - area.y)
                / (ui.global::<Terminal>().get_font_size() * 1.36).ceil())
            .max(0.) as usize;
            drop(scene);
            for _ in 0..lines.unsigned_abs().min(20) {
                self.terminal.mouse_report(
                    if lines > 0 { 64 } else { 65 },
                    col.min(size.cols - 1),
                    row.min(size.lines - 1),
                    false,
                );
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && !mods.shift {
            let bytes = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
            s.write(bytes.repeat(lines.unsigned_abs().min(20) as usize));
        } else {
            s.term.lock().scroll_display(Scroll::Delta(lines));
            self.terminal.dirty();
        }
        true
    }
}
impl super::State {
    fn mouse_report(&self, code: u8, col: usize, row: usize, release: bool) {
        let Some(s) = self.current() else {
            return;
        };
        let sgr = s.term.lock().mode().contains(TermMode::SGR_MOUSE);
        if sgr {
            s.write(
                format!(
                    "\x1b[<{code};{};{}{}",
                    col + 1,
                    row + 1,
                    if release { 'm' } else { 'M' }
                )
                .into_bytes(),
            );
        } else if col < 223 && row < 223 {
            s.write(vec![
                27,
                b'[',
                b'M',
                32 + if release { 3 } else { code },
                (col + 33) as u8,
                (row + 33) as u8,
            ]);
        }
    }
}
