//! Streaming installer transport and verified operation state; no timed percentages.
use crate::{AppWindow, Services};
use serde_json::{Value, json};
use slint::ComponentHandle;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::mpsc::Sender,
};

pub(super) fn run(events: &Sender<Value>) -> Result<String, String> {
    run_args(events, &["gui", "--progress"])
}

/// Updates one installed app (`key` is its Services row key, `app:ID`) with the same live progress.
pub(super) fn run_app(events: &Sender<Value>, key: &str) -> Result<String, String> {
    let id = key.strip_prefix("app:").filter(|id| !id.is_empty()).ok_or("Unknown app.")?;
    run_args(events, &["app", "update", id, "--progress"])
}

fn run_args(events: &Sender<Value>, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("/usr/bin/angi-update")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start the desktop installer: {error}"))?;
    let errors = child.stderr.take().unwrap();
    let send = events.clone();
    let error_reader = std::thread::spawn(move || {
        for line in BufReader::new(errors).lines().map_while(Result::ok) {
            let _ = send.send(json!({"log_line":line}));
        }
    });
    let mut terminal = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.map_err(|error| format!("Installer output failed: {error}"))?;
        match serde_json::from_str::<Value>(&line) {
            Ok(event) => {
                if matches!(event["state"].as_str(), Some("succeeded" | "failed")) {
                    terminal = Some(event.clone());
                }
                let _ = events.send(event);
            }
            Err(_) => {
                let _ = events.send(json!({"log_line":line}));
            }
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    let _ = error_reader.join();
    match terminal {
        Some(event) if status.success() && event["state"] == "succeeded" => Ok(event["message"]
            .as_str()
            .unwrap_or("Installation verified.")
            .to_string()),
        Some(event) => Err(event["message"]
            .as_str()
            .unwrap_or("Installation failed.")
            .to_string()),
        None => Err("The installer stopped without a verified completion result.".into()),
    }
}

pub(super) fn apply_receipt(ui: &AppWindow, receipt: &Value) {
    // Resume a running installer and surface its outcome once observed. A completed
    // receipt from a previous session is history, even if its release is still selected.
    if receipt["state"] == "running" || ui.global::<Services>().get_operation_state() == "running" {
        apply(ui, receipt);
    }
}

pub(super) fn apply(ui: &AppWindow, event: &Value) {
    let global = ui.global::<Services>();
    if let Some(line) = event["log_line"].as_str() {
        let log = format!("{}\n{line}", global.get_operation_log());
        global.set_operation_log(
            log.chars()
                .rev()
                .take(2400)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
                .into(),
        );
        return;
    }
    let Some(state) = event["state"].as_str() else {
        return;
    };
    if !matches!(state, "running" | "succeeded" | "failed") {
        return;
    }
    let previous = global.get_operation_state();
    if state == "running" && previous != "running" || previous == "running" && state != "running" {
        global.set_progress_open(true);
    }
    global.set_operation_state(state.into());
    global.set_operation_activation_pending(event["activation_pending"].as_bool().unwrap_or(false));
    global.set_operation_message(event["message"].as_str().unwrap_or("").into());
    global.set_operation_log(event["log"].as_str().unwrap_or("").into());
    global.set_operation_phase(event["phase"].as_str().unwrap_or("").into());
    global.set_operation_step(event["step"].as_i64().unwrap_or(0) as i32);
    global.set_operation_total(event["total"].as_i64().unwrap_or(7) as i32);
    global.set_operation_indeterminate(event["indeterminate"].as_bool().unwrap_or(false));
    global.set_operation_release(event["release"].as_str().unwrap_or("").into());
    global.set_operation_component(event["component"].as_str().unwrap_or("").into());
}
