//! Trash list and selection state. Filesystem work stays on the storage worker.
use super::worker::Worker;
use crate::{AppWindow, StorageTrashRow, StorageView, files::trash::Entry};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
pub(super) struct TrashPanel {
    entries: RefCell<Vec<Entry>>,
    selected: RefCell<Option<Entry>>,
}

impl TrashPanel {
    pub fn install(ui: &AppWindow, worker: Worker) -> Rc<Self> {
        let panel = Rc::new(Self::default());
        let (weak, list) = (ui.as_weak(), panel.clone());
        ui.global::<StorageView>().on_select_trash(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let view = ui.global::<StorageView>();
            if view.get_selected_open() || view.get_category() != 7 {
                return;
            }
            let entries = list.entries.borrow();
            let Some(entry) = entries.get(index as usize) else {
                return;
            };
            *list.selected.borrow_mut() = Some(entry.clone());
            view.set_selected_path(entry.stored.original.to_string_lossy().into_owned().into());
            view.set_selected_detail(
                entry
                    .bytes
                    .map(crate::system_ui::bytes)
                    .unwrap_or_default()
                    .into(),
            );
            view.set_selected_trashed(true);
            view.set_message("".into());
            view.set_confirming(false);
            view.set_selected_open(true);
        });
        let (weak, list, empty_worker) = (ui.as_weak(), panel.clone(), worker.clone());
        ui.global::<StorageView>().on_empty_trash(move || {
            let Some(ui) = weak.upgrade() else { return };
            let view = ui.global::<StorageView>();
            if !view.get_empty_confirming() || view.get_busy() { return; }
            let entries = list.entries.borrow().clone();
            if entries.is_empty() { return; }
            if empty_worker.empty_trash(entries) {
                view.set_busy(true);
            } else {
                view.set_trash_message("Trash is unavailable.".into());
            }
        });
        let (weak, list, restore_worker) = (ui.as_weak(), panel.clone(), worker.clone());
        ui.global::<StorageView>().on_restore_trash(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let view = ui.global::<StorageView>();
            if view.get_category() != 7 || view.get_busy() || view.get_trash_loading()
                || view.get_selected_open() || view.get_empty_confirming()
            {
                return;
            }
            if let Some(entry) = list.entries.borrow().get(index as usize).cloned() {
                if restore_worker.act_on_trash(entry, false) {
                    view.set_busy(true);
                    view.set_trash_message("Restoring item…".into());
                } else {
                    view.set_trash_message("Trash is unavailable.".into());
                }
            }
        });
        let (weak, list) = (ui.as_weak(), panel.clone());
        ui.global::<StorageView>().on_restore_file(move || {
            let Some(ui) = weak.upgrade() else { return };
            list.act(&ui, &worker, false);
        });
        panel
    }

    pub fn act(&self, ui: &AppWindow, worker: &Worker, permanent: bool) {
        let view = ui.global::<StorageView>();
        if !view.get_selected_open()
            || !view.get_selected_trashed()
            || view.get_busy()
            || view.get_confirming() != permanent
        {
            return;
        }
        if let Some(entry) = self.selected.borrow().clone() {
            if worker.act_on_trash(entry, permanent) {
                view.set_busy(true);
                view.set_message("".into());
            } else {
                view.set_message("Trash is unavailable.".into());
            }
        }
    }

    pub fn refresh(ui: &AppWindow, worker: &Worker) {
        let view = ui.global::<StorageView>();
        view.set_trash_message("".into());
        view.set_trash_loading(worker.list_trash());
        if !view.get_trash_loading() {
            view.set_trash_message("Trash is unavailable.".into());
        }
    }

    pub fn listed(&self, ui: &AppWindow, result: Result<Vec<Entry>, String>) {
        let view = ui.global::<StorageView>();
        view.set_trash_loading(false);
        let entries = match result {
            Ok(entries) => entries,
            Err(error) => {
                view.set_trash_message(format!("Could not read Trash: {error}").into());
                Vec::new()
            }
        };
        let rows = entries
            .iter()
            .map(|entry| {
                let path = &entry.stored.original;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let location = path
                    .parent()
                    .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
                StorageTrashRow {
                    name: name.into(),
                    location: location.into(),
                    size: entry
                        .bytes
                        .map(crate::system_ui::bytes)
                        .unwrap_or_default()
                        .into(),
                }
            })
            .collect::<Vec<_>>();
        *self.entries.borrow_mut() = entries;
        view.set_trash_items(ModelRc::new(VecModel::from(rows)));
    }

    pub fn acted(ui: &AppWindow, result: Result<(), String>) {
        let view = ui.global::<StorageView>();
        view.set_busy(false);
        view.set_confirming(false);
        match result {
            Ok(()) => {
                view.set_selected_open(false);
                ui.invoke_focus_desktop();
            }
            Err(error) => view.set_message(error.into()),
        }
    }
}
