//! The ANGI Chromium window. Chromium supplies its own browser chrome and page rendering.
mod native;
mod rfb;
mod session;

use crate::{
    App, AppWindow, ChromiumView,
    browser::keysym,
    input::Drag,
    keymap::Mods,
    wm::{BORDER, HEADER, Hit, Kind, Rect},
};
use rfb::Input;
use session::Session;
use slint::{ComponentHandle, Image, Timer, TimerMode};
use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

#[derive(Default)]
pub(super) struct Chromium {
    session: Option<Session>,
    native: native::Native,
    size: (u32, u32),
    buttons: u8,
    pointer: (u16, u16),
}

impl Chromium {
    fn send(&self, input: Input) {
        if let Some(session) = &self.session {
            let _ = session.input.try_send(input);
        }
    }
}

impl App {
    pub(super) fn chromium_open(&mut self, ui: &AppWindow, arguments: Vec<String>) {
        if self.chromium.native.available() {
            self.native_chromium_open(ui, arguments);
            return;
        }
        if cfg!(feature = "wayland-client")
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("SLINT_BACKEND").is_ok_and(|backend| backend.starts_with("winit-"))
        {
            match std::process::Command::new("/usr/bin/angi-chromium").args(arguments).spawn() {
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(error) => self.toast(ui, &format!("Could not launch Chromium: {error}")),
            }
            return;
        }
        if self.chromium.session.is_none() {
            if !Path::new("/usr/libexec/agx-wayland/session.py").is_file() {
                self.toast(ui, "Chromium needs the ANGI Wayland runtime.");
                return;
            }
            if !crate::library::bin_directory().join("chromium").is_file() {
                self.toast(ui, "Install Chromium from the ANGI Apps launcher first.");
                return;
            }
            self.chromium.session = Some(Session::launch(arguments));
            ui.global::<ChromiumView>()
                .set_message("Starting Chromium…".into());
        }
        self.desk.open(Kind::Chromium);
        self.refresh(ui);
    }

    pub(super) fn chromium_close(&mut self, ui: &AppWindow) {
        self.chromium.session = None;
        self.chromium.size = (0, 0);
        self.chromium.buttons = 0;
        ui.global::<ChromiumView>().set_frame(Image::default());
    }

    pub(super) fn chromium_key(&self, ui: &AppWindow, text: &str, mods: Mods, down: bool) -> bool {
        if self.desk.focused() != Some(Kind::Chromium) || self.modk.held(mods) || ui.get_menu_open()
        {
            return false;
        }
        if let Some(symbol) = keysym(text) {
            self.chromium.send(Input::Key(symbol, down));
        }
        true
    }

    pub(super) fn chromium_pointer(
        &mut self,
        ui: &AppWindow,
        event: i32,
        button: i32,
        x: f32,
        y: f32,
        mods: Mods,
    ) -> bool {
        if !matches!(self.drag, Drag::None) || self.modk.held(mods) {
            return false;
        }
        let scene = self.view.scene.borrow();
        if self.chromium.buttons == 0 && !matches!(scene.hit(x, y), Hit::Body(Kind::Chromium)) {
            return false;
        }
        let Some(rect) = scene.rect_of(Kind::Chromium) else {
            return false;
        };
        drop(scene);
        self.last = (x, y);
        ui.set_cursor(0);
        if event == 0 {
            self.desk.focus(Kind::Chromium);
            self.refresh(ui);
        }
        let (width, height) = self.chromium.size;
        if width == 0 || height == 0 {
            return true;
        }
        let viewport = viewport(rect, width, height);
        if self.chromium.buttons == 0 && !viewport.contains(x, y) {
            return true;
        }
        let x =
            ((x - viewport.x) * width as f32 / viewport.w).clamp(0.0, (width - 1) as f32) as u16;
        let y =
            ((y - viewport.y) * height as f32 / viewport.h).clamp(0.0, (height - 1) as f32) as u16;
        let mask = match button {
            0 => 1,
            1 => 4,
            2 => 2,
            _ => 0,
        };
        match event {
            0 => self.chromium.buttons |= mask,
            1 => self.chromium.buttons &= !mask,
            3 => self.chromium.buttons = 0,
            _ => {}
        }
        self.chromium.pointer = (x, y);
        self.chromium
            .send(Input::Pointer(x, y, self.chromium.buttons));
        true
    }

    pub(super) fn chromium_scroll(&self, dx: f32, dy: f32, mods: Mods) -> bool {
        let (x, y) = self.last;
        if self.modk.held(mods)
            || !matches!(
                self.view.scene.borrow().hit(x, y),
                Hit::Body(Kind::Chromium)
            )
        {
            return false;
        }
        let wheel = if dy > 0.0 {
            8
        } else if dy < 0.0 {
            16
        } else if dx > 0.0 {
            32
        } else if dx < 0.0 {
            64
        } else {
            0
        };
        if wheel != 0 {
            let (x, y) = self.chromium.pointer;
            let buttons = self.chromium.buttons;
            self.chromium.send(Input::Pointer(x, y, buttons | wheel));
            self.chromium.send(Input::Pointer(x, y, buttons));
        }
        true
    }
}

fn viewport(rect: Rect, width: u32, height: u32) -> Rect {
    let body = Rect::new(
        rect.x + BORDER,
        rect.y + HEADER,
        rect.w - 2.0 * BORDER,
        rect.h - HEADER - BORDER,
    );
    let scale = (body.w / width as f32).min(body.h / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    Rect::new(
        body.x + (body.w - w) / 2.0,
        body.y + (body.h - h) / 2.0,
        w,
        h,
    )
}

pub(super) fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Timer {
    let (weak, state) = (ui.as_weak(), app.clone());
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
        let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) else {
            return;
        };
        if app.native_chromium_tick(&ui) {
            return;
        }
        let Some(session) = &app.chromium.session else {
            return;
        };
        if let Ok(result) = session.result.try_recv() {
            let started = app.chromium.size != (0, 0);
            app.chromium_close(&ui);
            app.desk.close(Kind::Chromium);
            app.refresh(&ui);
            if !started {
                if let Err(error) = result {
                    eprintln!("Chromium: {error}");
                }
                app.toast(&ui, "Chromium could not start.");
            }
        } else if let Some(frame) = session.take_frame() {
            app.chromium.size = (frame.width(), frame.height());
            let view = ui.global::<ChromiumView>();
            view.set_message("".into());
            view.set_frame(Image::from_rgba8(frame));
        }
    });
    timer
}
