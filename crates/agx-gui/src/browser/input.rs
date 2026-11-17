//! Translate desktop content input without interfering with title bars or Super shortcuts.
use super::{TOOLBAR, protocol, viewport};
use crate::{
    App, AppWindow, Web,
    input::Drag,
    keymap::Mods,
    wm::{BORDER, HEADER, Hit, Kind},
};
use slint::ComponentHandle;
impl App {
    pub(crate) fn browser_shortcut(
        &mut self,
        ui: &AppWindow,
        name: &str,
        mods: Mods,
    ) -> Option<bool> {
        if self.desk.focused() != Some(Kind::Web) || self.modk.held(mods) {
            return None;
        }
        if ui.global::<Web>().get_popup_kind() != 0 && !(mods.alt && name == "tab") {
            return Some(false);
        }
        if name == "escape" && ui.global::<Web>().get_address_focused() {
            let uri = if self.browser.uri.is_empty() {
                protocol::HOME
            } else {
                &self.browser.uri
            };
            ui.global::<Web>().set_address(uri.into());
            self.browser.focus_address(ui, false);
            return Some(true);
        }
        if mods.alt && (name == "left" || name == "right") {
            self.browser_action(ui, if name == "left" { 0 } else { 1 });
            return Some(true);
        }
        if mods.ctrl && name == "l" {
            self.browser.focus_address(ui, true);
            return Some(true);
        }
        if mods.ctrl && name == "r" {
            self.browser_action(ui, 2);
            return Some(true);
        }
        // Plain digits and slash belong to the web page/address rather than desktop shortcuts.
        if !(mods.alt && name == "tab") {
            return Some(false);
        }
        None
    }
    pub(crate) fn browser_key(
        &mut self,
        ui: &AppWindow,
        text: &str,
        mods: Mods,
        down: bool,
    ) -> bool {
        if self.desk.focused() != Some(Kind::Web)
            || self.modk.held(mods)
            || ui.global::<Web>().get_address_focused()
            || ui.get_menu_open()
            || ui.global::<Web>().get_popup_kind() != 0
        {
            return false;
        }
        let Some(keysym) = protocol::keysym(text) else {
            return false;
        };
        let mut data = vec![u8::from(down)];
        data.extend(keysym.to_be_bytes());
        data.extend(protocol::modifiers(mods).to_be_bytes());
        self.browser.send(9, data);
        true
    }
    pub(crate) fn browser_pointer(
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
        let body = matches!(scene.hit(x, y), Hit::Body(Kind::Web));
        if !body && !self.browser.captured {
            return false;
        }
        let Some(placed) = scene
            .windows
            .iter()
            .find(|p| p.kind == Kind::Web && !p.collapsed)
        else {
            return false;
        };
        let rect = placed.rect;
        drop(scene);
        self.last = (x, y);
        self.cursor = if y < rect.y + HEADER + TOOLBAR {
            if x - rect.x - BORDER >= 144.0 { 7 } else { 6 }
        } else {
            self.browser.cursor
        };
        ui.set_cursor(self.cursor);
        if kind == 0 {
            self.desk.focus(Kind::Web);
        }
        let page = viewport(rect);
        if body && y < rect.y + HEADER + TOOLBAR && !self.browser.captured {
            if kind == 0 && button == 0 {
                let local = x - rect.x - BORDER;
                if (8.0..136.0).contains(&local) {
                    self.browser_action(ui, ((local - 8.0) / 32.0) as i32);
                } else if local >= 144.0 {
                    self.browser.focus_address(ui, true);
                }
            }
            if kind == 0 {
                self.refresh(ui);
            }
            return true;
        }
        let x = (x - page.x).clamp(0., page.w - 1.) as u16;
        let y = (y - page.y).clamp(0., page.h - 1.) as u16;
        if kind == 0 {
            self.browser.focus_address(ui, false);
            self.browser.captured = true;
            self.refresh(ui);
        }
        if kind == 1 || kind == 3 {
            self.browser.captured = false;
        }
        if kind == 3 {
            self.browser.buttons = 0;
        }
        let event = match kind {
            0 => 1,
            1 | 3 => 2,
            _ => 0,
        };
        let button = match button {
            0 => 1,
            1 => 3,
            2 => 2,
            _ => 0,
        };
        let mut data = vec![event, button];
        data.extend(x.to_be_bytes());
        data.extend(y.to_be_bytes());
        let mask = if button > 0 { 1 << (7 + button) } else { 0 };
        if event == 1 {
            self.browser.buttons |= mask;
        }
        if event == 2 {
            self.browser.buttons &= !mask;
        }
        self.browser.captured = self.browser.buttons != 0;
        data.extend((protocol::modifiers(mods) | self.browser.buttons).to_be_bytes());
        self.browser.send(8, data);
        true
    }
    pub(crate) fn browser_scroll(&self, dx: f32, delta: f32, mods: Mods) -> bool {
        let (x, y) = self.last;
        let scene = self.view.scene.borrow();
        if !matches!(scene.hit(x, y), Hit::Body(Kind::Web)) {
            return false;
        }
        let Some(rect) = scene.rect_of(Kind::Web).map(viewport) else {
            return false;
        };
        if !rect.contains(x, y) {
            return false;
        }
        let mut data = dx.to_be_bytes().to_vec();
        data.extend(delta.to_be_bytes());
        data.extend(((x - rect.x) as u16).to_be_bytes());
        data.extend(((y - rect.y) as u16).to_be_bytes());
        data.extend(protocol::modifiers(mods).to_be_bytes());
        self.browser.send(10, data);
        true
    }
}
