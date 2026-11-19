//! Host work review within the Agent Angi window and route its native actions.
mod view;

use crate::{
    AgentWork, App, AppWindow, files,
    keymap::{Cmd, Mods},
    wm::Kind,
};
use serde_json::Value;
use slint::{ComponentHandle, Image};
use std::{
    cell::RefCell,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use view::{Update, View};

#[derive(Default)]
pub(super) struct WorkEditor {
    server: Option<Server>,
    pending: Option<(String, Instant)>,
    view: Option<View>,
}

struct Server {
    process: Child,
    updates: Receiver<Value>,
}

impl Server {
    fn start() -> std::io::Result<Self> {
        let mut process = Command::new("/usr/bin/agent")
            .arg("work-editor")
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let reader = BufReader::new(process.stdout.take().unwrap());
        let (send, updates) = mpsc::channel();
        std::thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(event) = serde_json::from_str(&line)
                    && send.send(event).is_err()
                {
                    break;
                }
            }
        });
        Ok(Self { process, updates })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl WorkEditor {
    pub(super) fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Rc<RefCell<Self>> {
        let editor = Rc::new(RefCell::new(Self::default()));
        let work = ui.global::<AgentWork>();
        let (weak, state) = (ui.as_weak(), editor.clone());
        work.on_back(move || {
            let (Some(ui), Ok(mut state)) = (weak.upgrade(), state.try_borrow_mut()) else {
                return;
            };
            if let Some(view) = &mut state.view {
                if ui.global::<AgentWork>().get_dialog().is_empty() {
                    view.back();
                }
            } else {
                state.back(&ui);
            }
        });

        let (weak, state) = (ui.as_weak(), editor.clone());
        work.on_answer(move |yes| {
            let (Some(ui), Ok(mut state)) = (weak.upgrade(), state.try_borrow_mut()) else {
                return;
            };
            if let Some(view) = &mut state.view {
                view.answer(yes);
                ui.global::<AgentWork>().set_dialog("".into());
            }
        });

        let (state, app_state) = (editor.clone(), app.clone());
        work.on_key(move |text, c, a, s, m, down| {
            let mods = Mods {
                ctrl: c,
                alt: a,
                shift: s,
                meta: m,
            };
            if app_state.borrow().modk.held(mods) || (a && text == "\t") {
                return false;
            }
            state
                .borrow()
                .view
                .as_ref()
                .is_some_and(|view| view.key(&text, mods, down))
        });

        let (state, app_state) = (editor.clone(), app.clone());
        work.on_pointer(move |kind, button, x, y, c, a, s, m| {
            let mods = Mods {
                ctrl: c,
                alt: a,
                shift: s,
                meta: m,
            };
            if !app_state.borrow().modk.held(mods)
                && let Some(view) = &mut state.borrow_mut().view
            {
                view.pointer(kind, button, x, y, mods);
            }
        });

        let (state, app_state) = (editor.clone(), app.clone());
        work.on_scroll(move |x, y, c, a, s, m| {
            let mods = Mods {
                ctrl: c,
                alt: a,
                shift: s,
                meta: m,
            };
            if !app_state.borrow().modk.held(mods)
                && let Some(view) = &state.borrow().view
            {
                view.scroll(x, y, mods);
            }
        });
        editor
    }

    pub(super) fn select(&mut self, ui: &AppWindow, id: &str) {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return;
        }
        self.release(ui);
        match Server::start() {
            Ok(server) => {
                self.server = Some(server);
                self.pending = Some((id.to_owned(), Instant::now()));
            }
            Err(_) => self.failed(ui),
        }
    }

    pub(super) fn poll(&mut self, ui: &AppWindow, app: &Rc<RefCell<App>>) {
        let Some(server) = &mut self.server else {
            return;
        };
        while let Ok(event) = server.updates.try_recv() {
            if let (Some(port), Some(token)) = (event["port"].as_u64(), event["token"].as_str()) {
                if (1..=65535).contains(&port)
                    && !token.is_empty()
                    && token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    && let Some((id, _)) = self.pending.take()
                {
                    let uri = format!("http://127.0.0.1:{port}/#key={token}&id={id}");
                    match View::start(ui, uri) {
                        Ok(view) => self.view = Some(view),
                        Err(_) => {
                            self.failed(ui);
                            return;
                        }
                    }
                }
            } else if let Some(folder) = event["folder"].as_str() {
                files::open_folder(ui, app, folder);
            } else if event["refresh"].as_bool() == Some(true) {
                app.borrow_mut().run(ui, Cmd::RefreshDesktop);
            }
        }
        if !matches!(server.process.try_wait(), Ok(None))
            || self
                .pending
                .as_ref()
                .is_some_and(|(_, start)| start.elapsed() > Duration::from_secs(6))
        {
            self.failed(ui);
            return;
        }
        let (visible, active) = {
            let app = app.borrow();
            let scene = app.view.scene.borrow();
            let visible = scene
                .windows
                .iter()
                .any(|w| w.kind == Kind::AgentAngi && !w.collapsed);
            let active = app.desk.focused() == Some(Kind::AgentAngi)
                && !ui.get_menu_open()
                && !ui.get_session_open()
                && ui.get_confirm_text().is_empty()
                && ui.global::<AgentWork>().get_dialog().is_empty();
            (visible, active)
        };
        if let Some(view) = &mut self.view {
            match view.poll(ui, visible, visible && active) {
                Update::Running => {}
                Update::Back => self.back(ui),
                Update::Failed => self.failed(ui),
            }
        }
    }

    fn release(&mut self, ui: &AppWindow) {
        self.pending = None;
        self.view = None;
        self.server = None;
        let work = ui.global::<AgentWork>();
        work.set_ready(false);
        work.set_cursor(0);
        work.set_frame(Image::default());
        work.set_error("".into());
        work.set_dialog("".into());
    }

    fn back(&mut self, ui: &AppWindow) {
        self.release(ui);
        ui.global::<AgentWork>().set_open(false);
    }

    fn failed(&mut self, ui: &AppWindow) {
        self.release(ui);
        ui.global::<AgentWork>().set_error(
            "Could not load the ANGI work editor. Go back and reopen this record to retry.".into(),
        );
    }
}
