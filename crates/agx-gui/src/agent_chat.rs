//! Agent Angi's Chat mode: bubbles in the prompt panel, one `agent chat-turn` per message.
//!
//! The conversation lives in the desktop (`AgentChat.messages`); the agent runtime keeps the model's
//! own copy of it in an OpenCode session. The first message of each conversation asks for a new
//! session, so restarting the desktop never silently continues an older chat. Each turn runs on a
//! worker thread, so the UI never waits for the model.
use crate::{AgentChat, AgentScope, AppWindow, ChatMessage};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::Cell, process::{Command, Stdio}, rc::Rc, sync::mpsc, time::Duration};

/// What the agent runtime answered: Angi's text, or why there is none.
fn parse_reply(stdout: &[u8]) -> Result<String, String> {
    let answer: Value = serde_json::from_slice(stdout).map_err(|_| "Angi's runtime did not answer.".to_string())?;
    match (answer["text"].as_str(), answer["error"].as_str()) {
        (Some(text), _) => Ok(text.to_string()),
        (None, Some(error)) => Err(error.to_string()),
        _ => Err("Angi's runtime did not answer.".into()),
    }
}

fn ask(message: &str, new: bool, folder: &str) -> Result<String, String> {
    let mut command = Command::new("/usr/bin/agent");
    command.arg("chat-turn");
    if new {
        command.arg("--new");
    }
    if !folder.is_empty() {
        command.args(["--folder", folder]);
    }
    let output = command
        .arg(message)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "Agent Angi's runtime is not available.".to_string())?;
    parse_reply(&output.stdout)
}

fn bubble(mine: bool, failed: bool, text: &str) -> ChatMessage {
    ChatMessage { mine, failed, text: text.into() }
}

pub(super) fn install(ui: &AppWindow) -> Timer {
    let messages = Rc::new(VecModel::<ChatMessage>::default());
    ui.global::<AgentChat>().set_messages(ModelRc::from(messages.clone()));
    let (requests, worker) = mpsc::channel::<(String, bool, String)>();
    let (replies, answers) = mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        for (message, new, folder) in worker {
            if replies.send(ask(&message, new, &folder)).is_err() {
                break;
            }
        }
    });
    // The next message starts a new session until one has been sent in this conversation.
    let fresh = Rc::new(Cell::new(true));
    let (weak, model, fresh_send) = (ui.as_weak(), messages.clone(), fresh.clone());
    ui.global::<AgentChat>().on_send(move |text| {
        let Some(ui) = weak.upgrade() else { return };
        let (chat, text) = (ui.global::<AgentChat>(), text.trim());
        if chat.get_busy() || text.is_empty() {
            return;
        }
        model.push(bubble(true, false, text));
        chat.set_busy(true);
        let folder = ui.global::<AgentScope>().get_chat_folder().to_string();
        let _ = requests.send((text.to_string(), fresh_send.replace(false), folder));
    });
    let (weak, model) = (ui.as_weak(), messages.clone());
    ui.global::<AgentChat>().on_new_chat(move || {
        let Some(ui) = weak.upgrade() else { return };
        if !ui.global::<AgentChat>().get_busy() {
            model.set_vec(Vec::new());
            fresh.set(true);
        }
    });
    let (weak, timer) = (ui.as_weak(), Timer::default());
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(ui) = weak.upgrade() else { return };
        while let Ok(answer) = answers.try_recv() {
            ui.global::<AgentChat>().set_busy(false);
            match answer {
                Ok(text) => messages.push(bubble(false, false, &text)),
                Err(reason) => messages.push(bubble(false, true, &reason)),
            }
        }
    });
    timer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_text_or_a_reason() {
        assert_eq!(parse_reply(br#"{"text":"Hey there!"}"#), Ok("Hey there!".into()));
        assert_eq!(parse_reply(br#"{"error":"No credits"}"#), Err("No credits".into()));
        assert!(parse_reply(b"").is_err());
        assert!(parse_reply(b"{}").is_err());
    }

    #[test]
    fn a_failed_turn_is_marked_so_it_is_never_taken_for_angi() {
        let failure = bubble(false, true, "No credits");
        assert!(failure.failed && !failure.mine);
        let mine = bubble(true, false, "hi");
        assert!(mine.mine && !mine.failed);
    }
}
