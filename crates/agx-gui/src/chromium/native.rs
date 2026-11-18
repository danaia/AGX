//! Synchronize ANGI's Chromium tile with a native Wayland compositor shell.
use crate::{
    App, AppWindow,
    keymap::Cmd,
    wm::{Full, Kind},
};
use std::{fs, path::PathBuf, process::Command};

#[derive(Default)]
pub(super) struct Native {
    directory: Option<PathBuf>,
    layout: Option<[i32; 7]>,
    mapped: bool,
    event: u32,
}

impl Native {
    pub(super) fn available(&mut self) -> bool {
        if self.directory.is_none()
            && cfg!(feature = "wayland-client")
            && std::env::var_os("AGX_NATIVE_WAYLAND").is_some()
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            self.directory = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        }
        self.directory.is_some()
    }
}

impl App {
    pub(super) fn native_chromium_open(&mut self, ui: &AppWindow, arguments: Vec<String>) {
        if !self.chromium.native.mapped || !arguments.is_empty() {
            match Command::new("/usr/bin/angi-chromium")
                .args(arguments)
                .spawn()
            {
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(error) => {
                    self.toast(ui, &format!("Could not launch Chromium: {error}"));
                    return;
                }
            }
        }
        self.desk.open(Kind::Chromium);
        self.refresh(ui);
    }

    pub(super) fn native_chromium_tick(&mut self, ui: &AppWindow) -> bool {
        if !self.chromium.native.available() {
            return false;
        }
        let directory = self
            .chromium
            .native
            .directory
            .clone()
            .expect("native runtime");
        if let Ok(text) = fs::read_to_string(directory.join("angi-compositor-state")) {
            let mut fields = text.split_whitespace();
            let mapped = fields.next() == Some("1");
            if mapped != self.chromium.native.mapped {
                self.chromium.native.mapped = mapped;
                if mapped {
                    if !self.desk.is_open(Kind::Chromium) {
                        self.desk.open(Kind::Chromium);
                    }
                } else {
                    self.desk.close(Kind::Chromium);
                }
                self.refresh(ui);
            }
            let serial = fields
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            if serial != self.chromium.native.event {
                self.chromium.native.event = serial;
                match fields.next() {
                    Some("browser") if !ui.get_menu_open() => {
                        self.desk.focus(Kind::Chromium);
                        self.refresh(ui);
                    }
                    Some("menu") => self.run(ui, Cmd::Menu),
                    Some("close") => self.run(ui, Cmd::Close),
                    Some("float") => self.run(ui, Cmd::ToggleFloat),
                    Some("full") => self.run(ui, Cmd::Fullscreen),
                    Some(event @ ("maximize" | "fullscreen" | "restore")) => {
                        self.desk.focus(Kind::Chromium);
                        let current = self
                            .desk
                            .fullscreen_of()
                            .and_then(|(kind, mode)| (kind == Kind::Chromium).then_some(mode));
                        let requested = match event {
                            "maximize" => Some(Full::Maximized),
                            "fullscreen" => Some(Full::Screen),
                            _ => None,
                        };
                        if requested != current
                            && let Some(mode) = requested.or(current)
                        {
                            self.desk.toggle_fullscreen(mode);
                        }
                        self.refresh(ui);
                    }
                    Some("minimize") => {
                        if !self.desk.is_collapsed(Kind::Chromium) {
                            self.desk.toggle_collapsed(Kind::Chromium);
                            self.refresh(ui);
                        }
                    }
                    Some("layout") => self.run(ui, Cmd::CycleLayout),
                    Some(number) => {
                        if let Ok(index @ 0..=4) = number.parse() {
                            self.run(ui, Cmd::Workspace(index));
                        }
                    }
                    None => {}
                }
            }
        }

        let scene = self.view.scene.borrow();
        let mut layout = [
            i32::from(self.desk.is_open(Kind::Chromium)) * 2,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        if let Some((index, panel)) = scene
            .windows
            .iter()
            .enumerate()
            .find(|(_, panel)| panel.kind == Kind::Chromium && !panel.collapsed)
            && !ui.get_menu_open()
            && ui.get_confirm_text().is_empty()
        {
            let rect = panel.rect;
            let obscured = scene.windows[index + 1..].iter().any(|window| {
                let other = window.rect;
                rect.x < other.right()
                    && other.x < rect.right()
                    && rect.y < other.bottom()
                    && other.y < rect.bottom()
            });
            if !obscured {
                let mode = match self.desk.fullscreen_of() {
                    Some((Kind::Chromium, Full::Maximized)) => 1,
                    Some((Kind::Chromium, Full::Screen)) => 2,
                    _ => 0,
                };
                layout = [
                    1,
                    rect.x.round() as i32,
                    rect.y.round() as i32,
                    rect.w.round() as i32,
                    rect.h.round() as i32,
                    i32::from(panel.focused),
                    mode,
                ];
            }
        }
        if self.chromium.native.layout != Some(layout) {
            let temporary = directory.join("angi-compositor-layout.new");
            let [state, x, y, width, height, focus, mode] = layout;
            if fs::write(
                &temporary,
                format!("{state} {x} {y} {width} {height} {focus} {mode}\n"),
            )
            .and_then(|()| fs::rename(&temporary, directory.join("angi-compositor-layout")))
            .is_ok()
            {
                self.chromium.native.layout = Some(layout);
            }
        }
        true
    }
}
