//! Select an existing data partition and call the installed administrator helper off the UI thread.
use crate::{AppWindow, DriveMount};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::process::Command;

const HELPER: &str = "/usr/libexec/agx-agent/data_volume.py";

fn run(args: &[&str]) -> Result<String, String> {
    let output = Command::new("/usr/bin/angi-root")
        .args(["/usr/bin/python3", HELPER])
        .args(args)
        .output()
        .map_err(|error| format!("Could not start the ANGI drive helper: {error}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(message.trim().to_owned());
    }
    String::from_utf8(output.stdout).map_err(|_| "The drive helper returned invalid text.".into())
}

pub(super) fn install(ui: &AppWindow, worker: super::worker::Worker) -> slint::Timer {
    let partitions = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let weak = ui.as_weak();
    let list = partitions.clone();
    let page = ui.global::<DriveMount>();
    // The records are displayed and selected in the same order throughout the dialog.
    let (send, receive) = std::sync::mpsc::channel();
    page.on_show_picker(move || {
        let Some(ui) = weak.upgrade() else { return };
        let page = ui.global::<DriveMount>();
        if page.get_open() { return; }
        page.set_open(true);
        page.set_busy(true);
        page.set_choices(ModelRc::default());
        page.set_message("Looking for NVMe partitions…".into());
        let send = send.clone();
        std::thread::spawn(move || {
            let outcome = run(&["list"]).and_then(|data| {
                serde_json::from_str::<Vec<Value>>(&data)
                    .map_err(|_| "The drive helper returned an invalid partition list.".into())
            });
            let _ = send.send(outcome);
        });
    });
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(150), move || {
        let Ok(outcome) = receive.try_recv() else { return };
        let Some(ui) = weak.upgrade() else { return };
        let page = ui.global::<DriveMount>();
        page.set_busy(false);
        match outcome {
            Ok(entries) => {
                let (ids, labels): (Vec<_>, Vec<_>) = entries.into_iter().filter_map(|entry| {
                    let uuid = entry.get("uuid")?.as_str()?.to_owned();
                    let path = entry.get("path")?.as_str()?;
                    let size = crate::system_ui::bytes(entry.get("bytes")?.as_u64()?);
                    let label = entry.get("label").and_then(Value::as_str).unwrap_or_default();
                    let name = if label.is_empty() { format!("{path} · {size}") } else { format!("{label} · {path} · {size}") };
                    Some((uuid, SharedString::from(name)))
                }).unzip();
                page.set_message(if labels.is_empty() { "No unmounted ext4 NVMe partition was found." } else { "" }.into());
                *list.borrow_mut() = ids;
                page.set_choices(ModelRc::new(VecModel::from(labels)));
            }
            Err(error) => page.set_message(error.into()),
        }
    });
    let weak = ui.as_weak();
    page.on_mount(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let page = ui.global::<DriveMount>();
        if !page.get_open() || page.get_busy() || page.get_choices().row_data(index as usize).is_none() { return; }
        let Some(uuid) = partitions.borrow().get(index as usize).cloned() else { return };
        page.set_busy(true);
        page.set_message("Mounting the selected partition…".into());
        let weak = ui.as_weak();
        let worker = worker.clone();
        std::thread::spawn(move || {
            let outcome = run(&["mount", &uuid]);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let page = ui.global::<DriveMount>();
                page.set_busy(false);
                match outcome {
                    Ok(_) => {
                        page.set_open(false);
                        let _ = worker.refresh();
                        ui.invoke_focus_desktop();
                    }
                    Err(error) => page.set_message(error.into()),
                }
            });
        });
    });
    timer
}
