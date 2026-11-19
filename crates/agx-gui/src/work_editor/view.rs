//! A single offscreen Monaco surface and its input state.
use crate::{
    AgentWork, AppWindow,
    browser::{engine::Engine, protocol},
    keymap::Mods,
};
use slint::{ComponentHandle, Image};
use std::time::{Duration, Instant};

pub(super) struct View {
    engine: Engine,
    uri: Option<String>,
    created: Instant,
    size: (u16, u16),
    visible: bool,
    focused: bool,
    position: (u16, u16),
    buttons: u32,
    closing: bool,
}

pub(super) enum Update {
    Running,
    Back,
    Failed,
}

impl View {
    pub(super) fn start(ui: &AppWindow, uri: String) -> std::io::Result<Self> {
        Ok(Self {
            engine: Engine::start_embedded(ui)?,
            uri: Some(uri),
            created: Instant::now(),
            size: (0, 0),
            visible: false,
            focused: false,
            position: (0, 0),
            buttons: 0,
            closing: false,
        })
    }

    pub(super) fn poll(&mut self, ui: &AppWindow, visible: bool, focused: bool) -> Update {
        let updates = std::mem::take(&mut *self.engine.updates.lock().unwrap());
        let work = ui.global::<AgentWork>();
        if updates.close_requested {
            return Update::Back;
        }
        if updates.dead
            || updates
                .error
                .as_ref()
                .is_some_and(|error| !error.is_empty())
            || (self.uri.is_some()
                && !updates.ready
                && self.created.elapsed() > Duration::from_secs(15))
        {
            return Update::Failed;
        }
        if let Some((kind, message, _)) = updates.dialog {
            if kind == 3 {
                work.set_dialog("Discard the unsaved edits in this work record?".into());
            } else if kind == 1 {
                work.set_dialog(message.into());
            } else {
                self.answer(false);
            }
        } else if updates.popup_closed {
            work.set_dialog("".into());
        }
        if updates.options.is_some() {
            self.engine.send(13, u16::MAX.to_be_bytes().to_vec());
        }
        if let Some(cursor) = updates.cursor {
            work.set_cursor(cursor.into());
        }

        if let Some(frame) = updates.frame {
            if visible {
                work.set_frame_width(frame.pixels.width() as i32);
                work.set_frame_height(frame.pixels.height() as i32);
                work.set_frame(Image::from_rgba8(frame.pixels));
                work.set_ready(true);
            }
            self.engine.send(11, frame.sequence.to_be_bytes().to_vec());
        }
        if self.uri.is_some() && !updates.ready {
            return Update::Running;
        }
        let width = work.get_viewport_width().clamp(1, protocol::WIDTH as i32) as u16;
        let height = work.get_viewport_height().clamp(1, protocol::HEIGHT as i32) as u16;
        if visible && (width, height) != self.size {
            self.size = (width, height);
            let mut size = width.to_be_bytes().to_vec();
            size.extend(height.to_be_bytes());
            self.engine.send(1, size);
        }
        if visible != self.visible {
            self.visible = visible;
            self.engine.send(6, vec![u8::from(visible)]);
        }
        if focused != self.focused {
            self.focused = focused;
            self.engine.send(7, vec![u8::from(focused)]);
        }
        if let Some(uri) = self.uri.take() {
            self.engine.send(2, uri.into_bytes());
        }
        Update::Running
    }

    pub(super) fn back(&mut self) {
        if !self.closing {
            self.closing = true;
            self.engine.send(15, vec![]);
        }
    }

    pub(super) fn answer(&mut self, confirmed: bool) {
        if !confirmed {
            self.closing = false;
        }
        self.engine.send(14, vec![u8::from(confirmed)]);
    }

    pub(super) fn key(&self, text: &str, mods: Mods, down: bool) -> bool {
        let Some(keysym) = protocol::keysym(text) else {
            return false;
        };
        let mut data = vec![u8::from(down)];
        data.extend(keysym.to_be_bytes());
        data.extend(protocol::modifiers(mods).to_be_bytes());
        self.engine.send(9, data);
        true
    }

    pub(super) fn pointer(&mut self, kind: i32, button: i32, x: f32, y: f32, mods: Mods) {
        let (width, height) = self.size;
        self.position = (
            x.clamp(0.0, width.saturating_sub(1).into()) as u16,
            y.clamp(0.0, height.saturating_sub(1).into()) as u16,
        );
        let event = match kind {
            0 => 1,
            1 | 3 => 2,
            _ => 0,
        };
        let button = match button {
            0 if event != 0 => 1,
            1 if event != 0 => 3,
            2 if event != 0 => 2,
            _ => 0,
        };
        let mask = if button == 0 { 0 } else { 1 << (7 + button) };
        match kind {
            0 => self.buttons |= mask,
            1 => self.buttons &= !mask,
            3 => self.buttons = 0,
            _ => {}
        }
        let mut data = vec![event, button];
        data.extend(self.position.0.to_be_bytes());
        data.extend(self.position.1.to_be_bytes());
        data.extend((protocol::modifiers(mods) | self.buttons).to_be_bytes());
        self.engine.send(8, data);
    }

    pub(super) fn scroll(&self, dx: f32, dy: f32, mods: Mods) {
        let mut data = dx.to_be_bytes().to_vec();
        data.extend(dy.to_be_bytes());
        data.extend(self.position.0.to_be_bytes());
        data.extend(self.position.1.to_be_bytes());
        data.extend(protocol::modifiers(mods).to_be_bytes());
        self.engine.send(10, data);
    }
}
