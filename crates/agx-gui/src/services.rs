//! Services inventory and guarded desktop updates run outside the UI thread.
mod progress;
#[cfg(test)]
pub(super) mod tests;

use crate::{AppWindow, ServiceRow, Services};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

fn run(action: &str) -> Result<String, String> {
    let output = Command::new("/usr/bin/angi-update")
        .arg(action)
        .output()
        .map_err(|error| format!("Updater unavailable: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        let log = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "Update failed: {}",
            if error.trim().is_empty() {
                log.trim()
            } else {
                error.trim()
            }
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn install(ui: &AppWindow) -> Timer {
    // (action, row key): "check", "gui" for the desktop, "app" for an installed app's update, or "inventory".
    let (send, requests) = mpsc::sync_channel::<(&'static str, String)>(1);
    let (results, receive) = mpsc::channel();
    let (progress_send, progress_receive) = mpsc::channel();
    std::thread::spawn(move || {
        for (action, key) in requests {
            // Recheck availability in the worker; the confirmation may have been open
            // while a different deployment changed the installed release.
            let result = if action == "gui" {
                (|| {
                    let inventory: Value =
                        serde_json::from_str(&run("inventory")?).map_err(|e| e.to_string())?;
                    let available = inventory["rows"].as_array().is_some_and(|rows| {
                        rows.iter()
                            .any(|row| row["key"] == "desktop" && row["action"] == "Update")
                    });
                    if !available {
                        return Err("Desktop source is current or installation is blocked. Check for updates again.".into());
                    }
                    progress::run(&progress_send)
                })()
            } else if action == "app" {
                progress::run_app(&progress_send, &key)
            } else {
                Ok(String::new())
            };
            if let Err(error) = &result {
                if action == "gui" || action == "app" {
                    let _ =
                        progress_send.send(serde_json::json!({"state":"failed", "message":error}));
                }
            }
            let inventory = run(if action == "check" {
                "check-releases"
            } else {
                "inventory"
            })
            .and_then(|output| serde_json::from_str::<Value>(&output).map_err(|e| e.to_string()));
            if results.send((result, inventory)).is_err() {
                break;
            }
        }
    });
    for (kind, update) in [("check", false), ("gui", true)] {
        let (weak, sender) = (ui.as_weak(), send.clone());
        let callback = move |key: &str| {
            // "gui" is the update callback: the desktop builds from source, an app downloads a release.
            let action = match (update, key) {
                (false, _) => kind,
                (true, "desktop") => "gui",
                (true, key) if key.starts_with("app:") => "app",
                (true, _) => return,
            };
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let global = ui.global::<Services>();
            if global.get_busy() || global.get_operation_state() == "running" {
                return;
            }
            if sender.try_send((action, key.to_string())).is_ok() {
                global.set_busy(true);
                if update {
                    global.set_operation_state("running".into());
                    global.set_operation_component(
                        if action == "gui" { "ANGI desktop" } else { key.trim_start_matches("app:") }.into(),
                    );
                    global.set_operation_message(
                        if action == "gui" { "Starting desktop update…" } else { "Starting update…" }.into(),
                    );
                    global.set_operation_log("".into());
                    global.set_operation_step(0);
                    global.set_operation_indeterminate(true);
                    global.set_progress_open(true);
                }
                global.set_notice(
                    if action == "gui" {
                        "Building and installing desktop… This may take several minutes."
                    } else if update {
                        "Downloading and installing the update…"
                    } else {
                        "Checking upstream releases…"
                    }
                    .into(),
                );
            }
        };
        if update {
            ui.global::<Services>()
                .on_update(move |key| callback(key.as_str()));
        } else {
            ui.global::<Services>().on_check(move || callback(""));
        }
    }
    ui.global::<Services>().set_busy(true);
    let _ = send.try_send(("inventory", String::new()));
    let weak = ui.as_weak();
    let timer = Timer::default();
    let mut last_inventory = Instant::now();
    let mut last_restart_notice = String::new();
    let mut retry_pending = false;
    let mut poll_interval = Duration::from_secs(60);
    timer.start(TimerMode::Repeated, Duration::from_millis(200), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let global = ui.global::<Services>();
        while let Ok(event) = progress_receive.try_recv() {
            progress::apply(&ui, &event);
        }
        while let Ok((result, inventory)) = receive.try_recv() {
            global.set_busy(false);
            last_inventory = Instant::now();
            match inventory {
                Ok(value) => {
                    progress::apply_receipt(&ui, &value["update_job"]);
                    let retry = value["retry_after_seconds"].as_u64().unwrap_or(0);
                    retry_pending = retry > 0;
                    poll_interval = Duration::from_secs(if retry_pending {
                        retry.clamp(5, 60)
                    } else {
                        60
                    });
                    let rows = value["rows"]
                        .as_array()
                        .map(|rows| {
                            rows.iter()
                                .filter(|row| {
                                    row["update_state"] != "update"
                                        || row["action"]
                                            .as_str()
                                            .is_some_and(|action| !action.is_empty())
                                        || row["restart_pending"] == true
                                })
                                .map(|row| {
                                    let text = |key: &str| row[key].as_str().unwrap_or("").into();
                                    ServiceRow {
                                        restart_kind: text("restart_kind"),
                                        restart_label: text("restart_label"),
                                        restart_detail: text("restart_detail"),
                                        restart_pending: row["restart_pending"]
                                            .as_bool()
                                            .unwrap_or(false),
                                        install_note: text("install_note"),
                                        key: text("key"),
                                        name: text("name"),
                                        update_state: text("update_state"),
                                        update_label: text("update_label"),
                                        updated_ago: text("updated_ago"),
                                        checked_ago: text("checked_ago"),
                                        latest: text("latest"),
                                        state: text("state"),
                                        installed: text("installed"),
                                        status: text("status"),
                                        detail: text("detail"),
                                        action: text("action"),
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if let Some(pending) = rows.iter().find(|row| row.restart_pending) {
                        if pending.installed.as_str() != last_restart_notice
                            && !global.get_review_open()
                            && !global.get_progress_open()
                        {
                            last_restart_notice = pending.installed.to_string();
                            global.set_pending(pending.clone());
                            global.set_review_open(true);
                        }
                    }
                    if global.get_progress_open() {
                        if let Some(pending) = rows.iter().find(|row| row.restart_pending) {
                            last_restart_notice = pending.installed.to_string();
                        }
                    }
                    global.set_rows(ModelRc::new(VecModel::from(rows)));
                    let notice = match result {
                        Err(error) => error,
                        Ok(message) if !message.is_empty() => message,
                        _ => value["notice"]
                            .as_str()
                            .unwrap_or("Check complete")
                            .to_string(),
                    };
                    global.set_notice(notice.into());
                }
                Err(error) => global.set_notice(error.into()),
            }
        }
        if last_inventory.elapsed()
            >= if global.get_operation_state() == "running" {
                Duration::from_secs(2)
            } else {
                poll_interval
            }
            && !global.get_busy()
            && (global.get_operation_state() == "running"
                || retry_pending
                || ui
                    .get_wins()
                    .iter()
                    .any(|window| window.kind == 5 && !window.collapsed))
            && send.try_send(("inventory", String::new())).is_ok()
        {
            global.set_busy(true);
            last_inventory = Instant::now();
        }
    });
    timer
}
