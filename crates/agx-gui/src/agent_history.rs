//! Load conversation history from the agent runtime and restore the selected workspace.
use crate::{AgentChat, AgentHistory, AgentScope, AgentWork, App, AppWindow, HistoryEntry, WorkRecord, WorkResource, agent_chat, files};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::RefCell, process::Command, rc::Rc, sync::mpsc, time::Duration};

fn request(args: &[String]) -> Result<Value, String> {
    let output = Command::new("/usr/bin/agent")
        .arg("history")
        .args(args)
        .output()
        .map_err(|_| "Agent Angi’s runtime is not available.".to_string())?;
    let answer: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Chat history could not be read. Try again.".to_string())?;
    match answer["error"].as_str() {
        Some(error) => Err(error.to_string()),
        None => Ok(answer),
    }
}

pub(super) fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Timer {
    let (weak, app) = (ui.as_weak(), app.clone());
    ui.global::<AgentWork>().on_open_folder(move |path| {
        if let Some(ui) = weak.upgrade() {
            files::open_folder(&ui, &app, &path);
        }
    });
    let (requests, worker) = mpsc::channel::<Vec<String>>();
    let (results, updates) = mpsc::channel();
    std::thread::spawn(move || {
        for args in worker {
            let result = request(&args);
            if results.send((args.len() == 1, result)).is_err() {
                break;
            }
        }
    });
    let weak = ui.as_weak();
    let enqueue = move |args: Vec<String>| {
        if let Some(ui) = weak.upgrade() {
            let history = ui.global::<AgentHistory>();
            if !history.get_loading() && (args.is_empty() || !ui.global::<AgentChat>().get_busy()) {
                history.set_error("".into());
                history.set_loading(true);
                let _ = requests.send(args);
            }
        }
    };
    let history = ui.global::<AgentHistory>();
    let list = enqueue.clone();
    history.on_reload(move || list(Vec::new()));
    let open = enqueue.clone();
    history.on_choose(move |id| open(vec![id.into()]));
    let rename = enqueue.clone();
    history.on_rename(move |id, title| rename(vec!["rename".into(), id.into(), title.into()]));
    history.on_remove(move |id| enqueue(vec!["remove".into(), id.into()]));

    let (weak, timer) = (ui.as_weak(), Timer::default());
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(ui) = weak.upgrade() else { return; };
        let history = ui.global::<AgentHistory>();
        while let Ok((opening, result)) = updates.try_recv() {
            history.set_loading(false);
            match result {
                Ok(answer) if !opening => {
                    let entries: Vec<HistoryEntry> = answer["entries"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|entry| Some(HistoryEntry {
                            id: entry["id"].as_str()?.into(),
                            mode: entry["mode"].as_str()?.into(),
                            title: entry["title"].as_str()?.into(),
                        }))
                        .collect();
                    history.set_entries(ModelRc::new(VecModel::from(entries)));
                }
                Ok(answer) if history.get_open() => {
                    restore(&ui, &answer);
                    history.set_open(false);
                }
                Err(error) => history.set_error(error.into()),
                _ => {}
            }
        }
    });
    timer
}

fn restore(ui: &AppWindow, answer: &Value) {
    let scope = ui.global::<AgentScope>();
    let field = |key: &str| answer[key].as_str().unwrap_or("");
    match field("mode") {
        "chat" => {
            ui.global::<AgentWork>().set_open(false);
            let chat = ui.global::<AgentChat>();
            let messages = answer["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| Some(agent_chat::bubble(row["mine"].as_bool()?, false, row["text"].as_str()?)))
                .collect();
            agent_chat::set_messages(&chat, messages);
            chat.set_session_id(field("id").into());
            chat.set_draft("".into());
            scope.set_chat_folder(field("folder").into());
            scope.set_mode("chat".into());
        }
        "os" | "project" => {
            let resources: Vec<WorkResource> = answer["resources"].as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| Some(WorkResource {
                    label: row["label"].as_str()?.into(),
                    path: row["path"].as_str()?.into(),
                }))
                .collect();
            let work = ui.global::<AgentWork>();
            work.set_record(WorkRecord {
                mode: field("mode").into(),
                title: field("title").into(),
                brief: field("brief").into(),
                summary: field("summary").into(),
                summary_label: field("summary_label").into(),
                resources: ModelRc::new(VecModel::from(resources)),
            });
            scope.set_mode(field("mode").into());
            work.set_open(true);
        }
        _ => {}
    }
}
