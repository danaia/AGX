//! Browser lifecycle and desktop integration. WPE stays out of the GUI process.
mod engine;
mod input;
mod protocol;
mod storage;
use crate::{
    App, AppWindow, Web, WebOption,
    wm::{BORDER, HEADER, Kind, Rect},
};
use engine::Engine;
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
pub(super) use storage::{prepare_accounts, prepare_user_storage};
const TOOLBAR: f32 = 44.0;
#[derive(Default)]
pub(super) struct Browser {
    engine: Option<Engine>,
    attempted: bool,
    ready: bool,
    starting: Option<Instant>,
    closing: Option<Instant>,
    geometry: Option<(u16, u16)>,
    pending: Option<Instant>,
    visible: bool,
    focused: bool,
    pub captured: bool,
    buttons: u32,
    cursor: i32,
    uri: String,
}
fn viewport(rect: Rect) -> Rect {
    Rect::new(
        rect.x + BORDER,
        rect.y + HEADER + TOOLBAR,
        (rect.w - 2.0 * BORDER).clamp(1.0, protocol::WIDTH as f32),
        (rect.h - HEADER - TOOLBAR - BORDER).clamp(1.0, protocol::HEIGHT as f32),
    )
}
impl Browser {
    fn send(&self, kind: u8, data: Vec<u8>) {
        if let Some(engine) = &self.engine {
            engine.send(kind, data);
        }
    }
    fn set_geometry(&mut self, size: (u16, u16)) {
        let mut data = size.0.to_be_bytes().to_vec();
        data.extend(size.1.to_be_bytes());
        self.send(1, data);
        self.geometry = Some(size);
        self.pending = None;
    }
    fn update_geometry(&mut self, size: (u16, u16), now: Instant) {
        if self.geometry == Some(size) {
            self.pending = None;
            return;
        }
        let since = *self.pending.get_or_insert(now);
        if now.saturating_duration_since(since) >= Duration::from_millis(100) {
            self.set_geometry(size);
        }
    }
    fn reset(&mut self, ui: &AppWindow) {
        *self = Self::default();
        let web = ui.global::<Web>();
        web.set_frame(Image::default());
        web.set_frame_width(0);
        web.set_frame_height(0);
        web.set_ready(false);
        web.set_error("".into());
        web.set_address(protocol::HOME.into());
        web.set_site(protocol::HOME.into());
        web.set_title("Web".into());
        web.set_address_focused(false);
        web.set_loading(false);
        web.set_can_back(false);
        web.set_can_forward(false);
        web.set_popup_kind(0);
        web.set_popup_visible(false);
        web.set_options(ModelRc::new(VecModel::default()));
        web.set_dialog_message("".into());
        web.set_dialog_value("".into());
    }
    fn focus_address(&self, ui: &AppWindow, focused: bool) {
        let web = ui.global::<Web>();
        web.set_address_focused(focused);
        web.set_focus_serial(web.get_focus_serial().wrapping_add(1));
    }
}
impl App {
    pub(super) fn browser_tick(&mut self, ui: &AppWindow) {
        if !self.desk.is_open(Kind::Web) {
            if self.browser.attempted {
                self.browser.reset(ui);
            }
            return;
        }
        let web = ui.global::<Web>();
        let browser = &mut self.browser;
        if !browser.attempted {
            browser.attempted = true;
            web.set_error("".into());
            match Engine::start() {
                Ok(engine) => {
                    browser.engine = Some(engine);
                    browser.starting = Some(Instant::now());
                }
                Err(error) => web.set_error(format!("Could not start Web: {error}").into()),
            }
        }
        let scene = self.view.scene.borrow();
        let placed = scene
            .windows
            .iter()
            .find(|p| p.kind == Kind::Web && !p.collapsed);
        let visible = placed.is_some();
        web.set_popup_visible(
            visible
                && self.desk.focused() == Some(Kind::Web)
                && !ui.get_menu_open()
                && !ui.get_session_open()
                && ui.get_confirm_text().is_empty(),
        );
        if let Some(placed) = placed {
            web.set_popup_x(placed.rect.x + BORDER);
            web.set_popup_y(placed.rect.y + HEADER);
            web.set_popup_w((placed.rect.w - 2.0 * BORDER).max(0.0));
            web.set_popup_h((placed.rect.h - HEADER - BORDER).max(0.0));
        }
        let focused = visible
            && self.desk.focused() == Some(Kind::Web)
            && !web.get_address_focused()
            && web.get_popup_kind() == 0
            && !ui.get_menu_open()
            && !ui.get_session_open()
            && ui.get_confirm_text().is_empty();
        if let Some(engine) = &browser.engine {
            let mut updates = {
                let mut state = engine.updates.lock().unwrap();
                std::mem::take(&mut *state)
            };
            if updates.close_requested {
                drop(scene);
                self.desk.close(Kind::Web);
                self.browser.reset(ui);
                self.refresh(ui);
                return;
            }
            if updates.popup_closed {
                web.set_popup_kind(0);
            }
            if let Some(options) = updates.options {
                web.set_option_index(
                    options.iter().position(|item| item.selected).unwrap_or(0) as i32
                );
                web.set_options(ModelRc::new(VecModel::from(
                    options
                        .into_iter()
                        .map(|item| WebOption {
                            label: item.label.into(),
                            enabled: item.enabled,
                            selected: item.selected,
                        })
                        .collect::<Vec<_>>(),
                )));
                web.set_popup_kind(1);
            }
            if let Some((kind, message, value)) = updates.dialog {
                web.set_dialog_type(kind as i32);
                web.set_dialog_message(message.into());
                web.set_dialog_value(value.into());
                web.set_popup_kind(2);
            }
            if let Some(cursor) = updates.cursor {
                browser.cursor = cursor as i32;
                if placed.is_some_and(|p| viewport(p.rect).contains(self.last.0, self.last.1))
                    && matches!(
                        scene.hit(self.last.0, self.last.1),
                        crate::wm::Hit::Body(Kind::Web)
                    )
                {
                    self.cursor = browser.cursor;
                    ui.set_cursor(self.cursor);
                }
            }
            if updates.ready {
                browser.ready = true;
                browser.starting = None;
                if let Some(placed) = placed {
                    let rect = viewport(placed.rect);
                    let size = (rect.w.ceil() as u16, rect.h.ceil() as u16);
                    browser.set_geometry(size);
                }
                browser.send(6, vec![u8::from(visible)]);
                browser.visible = visible;
                browser.send(2, protocol::HOME.as_bytes().to_vec());
            }
            if let Some(frame) = updates.frame.take() {
                if visible {
                    web.set_frame_width(frame.pixels.width() as i32);
                    web.set_frame_height(frame.pixels.height() as i32);
                    web.set_frame(Image::from_rgba8(frame.pixels));
                    web.set_ready(true);
                }
                browser.send(11, frame.sequence.to_be_bytes().to_vec());
            }
            if let Some(title) = updates.title {
                web.set_title(if title.is_empty() {
                    "Web".into()
                } else {
                    title.into()
                });
            }
            if let Some(uri) = updates.uri {
                web.set_site(uri.clone().into());
                if !web.get_address_focused() {
                    web.set_address(uri.clone().into());
                }
                browser.uri = uri;
            }
            if let Some((flags, progress)) = updates.status {
                web.set_loading(flags & 1 != 0);
                web.set_can_back(flags & 2 != 0);
                web.set_can_forward(flags & 4 != 0);
                web.set_progress(progress as f32 / 100.0);
            }
            if let Some(error) = updates.error {
                web.set_error(error.into());
            }
            if updates.dead {
                browser.engine = None;
                web.set_popup_kind(0);
                browser.ready = false;
                browser.starting = None;
                web.set_loading(false);
            }
        }
        if browser
            .closing
            .is_some_and(|since| since.elapsed() > Duration::from_secs(2))
            && !(web.get_popup_kind() == 2 && web.get_dialog_type() == 3)
        {
            drop(scene);
            self.desk.close(Kind::Web);
            self.browser.reset(ui);
            self.refresh(ui);
            return;
        }
        if browser
            .starting
            .is_some_and(|since| since.elapsed() > Duration::from_secs(20))
        {
            browser.engine = None;
            browser.starting = None;
            web.set_error("Web took too long to start. Close and reopen it to retry.".into());
        }
        if !browser.ready {
            return;
        }
        if browser.visible != visible {
            browser.send(6, vec![u8::from(visible)]);
            browser.visible = visible;
        }
        if browser.focused != focused {
            browser.send(7, vec![u8::from(focused)]);
            browser.focused = focused;
        }
        if let Some(placed) = placed {
            let rect = viewport(placed.rect);
            let size = (rect.w.ceil() as u16, rect.h.ceil() as u16);
            browser.update_geometry(size, Instant::now());
        }
    }
    pub(super) fn browser_request_close(&mut self) -> bool {
        if !self.browser.ready {
            return false;
        }
        if self.browser.closing.is_none() {
            self.browser.closing = Some(Instant::now());
            self.browser.send(15, vec![]);
        }
        true
    }
    pub(super) fn browser_selection(&mut self, ui: &AppWindow, index: i32) {
        self.browser.captured = false;
        self.browser.buttons = 0;
        self.browser.send(13, (index as u16).to_be_bytes().to_vec());
        ui.global::<Web>().set_popup_kind(0);
        self.browser.focus_address(ui, false);
    }
    pub(super) fn browser_dialog(&mut self, ui: &AppWindow, accepted: bool, text: &str) {
        self.browser.captured = false;
        self.browser.buttons = 0;
        if ui.global::<Web>().get_dialog_type() == 3 && !accepted {
            self.browser.closing = None;
        }
        if text.len() > 8192 {
            return;
        }
        let mut data = vec![u8::from(accepted)];
        data.extend(text.as_bytes());
        self.browser.send(14, data);
        ui.global::<Web>().set_popup_kind(0);
        self.browser.focus_address(ui, false);
    }
    pub(super) fn browser_action(&mut self, ui: &AppWindow, action: i32) {
        match action {
            0 => self.browser.send(3, vec![]),
            1 => self.browser.send(4, vec![]),
            2 => self.browser.send(5, vec![]),
            3 => {
                ui.global::<Web>().set_address(protocol::HOME.into());
                self.browser.send(2, protocol::HOME.as_bytes().to_vec());
            }
            _ => return,
        }
        self.browser.focus_address(ui, false);
    }
    pub(super) fn browser_navigate(&mut self, ui: &AppWindow, address: &str) {
        match protocol::address(address) {
            Some(uri) => {
                ui.global::<Web>().set_address(uri.clone().into());
                ui.global::<Web>().set_error("".into());
                self.browser.send(2, uri.into_bytes());
                self.browser.focus_address(ui, false);
            }
            None => ui
                .global::<Web>()
                .set_error("Enter a website address, such as https://example.org".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_resizing_still_updates_the_page_at_a_bounded_rate() {
        let mut browser = Browser::default();
        browser.geometry = Some((800, 600));
        let now = Instant::now();
        browser.update_geometry((900, 600), now);
        browser.update_geometry((950, 600), now + Duration::from_millis(60));
        assert_eq!(browser.geometry, Some((800, 600)));
        browser.update_geometry((1000, 600), now + Duration::from_millis(110));
        assert_eq!(
            browser.geometry,
            Some((1000, 600)),
            "a continuing drag must not postpone every resize"
        );
        browser.update_geometry((1000, 600), now + Duration::from_millis(120));
        assert!(browser.pending.is_none());
    }
    #[test]
    fn viewport_matches_frame_and_toolbar_and_bounds_memory() {
        let r = viewport(Rect::new(50., 60., 800., 600.));
        assert_eq!(r, Rect::new(52., 132., 796., 526.));
        let r = viewport(Rect::new(0., 0., 9000., 9000.));
        assert_eq!((r.w, r.h), (4096., 2160.));
    }
}

/// Keep browser wiring and its timer alongside the lifecycle it drives.
pub(super) fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) -> slint::Timer {
    macro_rules! web_hook {
        ($callback:ident, |$state:ident, $ui:ident $(, $arg:ident)*| $body:expr) => {{
            let (weak, state) = (ui.as_weak(), app.clone());
            ui.global::<Web>().$callback(move |$($arg),*| {
                if let (Some($ui), Ok(mut $state)) = (weak.upgrade(), state.try_borrow_mut()) { $body }
            });
        }};
    }
    web_hook!(on_action, |state, ui, action| state
        .browser_action(&ui, action));
    web_hook!(on_navigate, |state, ui, address| state
        .browser_navigate(&ui, &address));
    web_hook!(on_selection, |state, ui, index| state
        .browser_selection(&ui, index));
    web_hook!(on_dialog_response, |state, ui, accepted, value| state
        .browser_dialog(&ui, accepted, &value));
    let (weak, state) = (ui.as_weak(), app.clone());
    ui.on_browser_key(move |text, ctrl, alt, shift, meta, down| {
        let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) else {
            return false;
        };
        app.browser_key(
            &ui,
            &text,
            crate::keymap::Mods {
                ctrl,
                alt,
                shift,
                meta,
            },
            down,
        )
    });
    let timer = slint::Timer::default();
    let (weak, state) = (ui.as_weak(), app.clone());
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(33),
        move || {
            if let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) {
                app.browser_tick(&ui);
            }
        },
    );
    timer
}
