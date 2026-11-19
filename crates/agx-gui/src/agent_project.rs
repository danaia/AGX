//! Folder selection for Agent Angi's Project and Chat modes.
//!
//! A folder reaches the prompt through two hooks on `AgentScope`: `choose-project` (the user asked
//! to pick one) and `use-project(path)` (a folder was chosen). `choose-project` opens ANGI Files'
//! folder picker (`ui/files-picker.slint`, `src/files/`); the picker calls `use-project` with its
//! choice. This module owns what happens next: the agent runtime validates every folder
//! (`agent project PATH`) and checks it again at launch, so the picker never needs to know the
//! rules. A rejected folder keeps the picker open with the reason; an accepted one closes it.
use crate::{AgentScope, AppWindow, Files};
use serde_json::Value;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{process::Command, sync::mpsc, time::Duration};

pub(super) fn install(ui: &AppWindow) -> Timer {
    let (results, updates) = mpsc::channel::<(bool, Value)>();
    let weak = ui.as_weak();
    ui.global::<AgentScope>().on_choose_project(move || {
        if let Some(ui) = weak.upgrade() {
            let scope = ui.global::<AgentScope>();
            scope.set_error("".into());
            let for_chat = scope.get_mode() == "chat";
            scope.set_picker_for_chat(for_chat);
            let folder = if for_chat { scope.get_chat_folder() } else { scope.get_project() };
            ui.global::<Files>().invoke_open_picker(folder);
        }
    });
    let weak = ui.as_weak();
    ui.global::<AgentScope>().on_use_project(move |path| {
        let Some(ui) = weak.upgrade() else { return };
        let scope = ui.global::<AgentScope>();
        if scope.get_checking() || path.trim().is_empty() {
            return;
        }
        scope.set_checking(true);
        scope.set_error("".into());
        let for_chat = scope.get_picker_for_chat();
        let (path, results) = (path.to_string(), results.clone());
        std::thread::spawn(move || {
            let answer = Command::new("/usr/bin/agent")
                .args(["project", &path])
                .output()
                .ok()
                .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
                .unwrap_or_else(|| serde_json::json!({"error": "Agent Angi's runtime is not available."}));
            let _ = results.send((for_chat, answer));
        });
    });
    let (weak, timer) = (ui.as_weak(), Timer::default());
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(ui) = weak.upgrade() else { return };
        let scope = ui.global::<AgentScope>();
        while let Ok((for_chat, answer)) = updates.try_recv() {
            scope.set_checking(false);
            let files = ui.global::<Files>();
            match (answer["path"].as_str(), answer["error"].as_str()) {
                (Some(path), _) => {
                    if for_chat {
                        scope.set_chat_folder(path.into());
                    } else {
                        scope.set_project(path.into());
                    }
                    scope.set_error("".into());
                    files.set_chooser_error("".into());
                    files.set_chooser_open(false);
                }
                (None, error) => {
                    // The reason belongs where the user is looking: the picker, if it is open.
                    let reason = error.unwrap_or("That folder cannot be used.");
                    if files.get_chooser_open() {
                        files.set_chooser_error(reason.into());
                    } else {
                        scope.set_error(reason.into());
                    }
                }
            }
        }
    });
    timer
}
