//! Storage inventory lifecycle and the heat map's Slint presentation model.
mod cache;
mod cache_dialog;
mod category;
mod delete;
mod mount;
mod policy;
mod risk;
mod scan;
#[cfg(test)]
mod tests;
mod trash_panel;
mod view;
mod worker;

use crate::{AppWindow, StorageView};
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc, sync::mpsc, time::Duration};

pub(super) struct Watch {
    worker: worker::Worker,
    _timer: slint::Timer,
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.worker.stop();
    }
}

pub(super) fn install(ui: &AppWindow) -> Watch {
    let policy = policy::Policy::current();
    let (publish, updates) = mpsc::channel();
    let worker = worker::Worker::start(policy, publish);
    mount::install(ui, worker.clone());
    let catalog = Rc::new(RefCell::new(view::State::default()));
    catalog.borrow().bind(ui);
    let selected = Rc::new(RefCell::new(None::<(scan::File, bool)>));
    let trash_panel = trash_panel::TrashPanel::install(ui, worker.clone());
    let cache_dialog = cache_dialog::install(ui, worker.clone());
    {
        let (weak, catalog, selected) = (ui.as_weak(), catalog.clone(), selected.clone());
        ui.global::<StorageView>()
            .on_select_file(move |drive, index| {
                let Some(ui) = weak.upgrade() else { return };
                let view = ui.global::<StorageView>();
                if view.get_selected_open() {
                    return;
                }
                let Some((file, verified_cache)) =
                    catalog.borrow().tile(drive as usize, index as usize)
                else {
                    return;
                };
                view.set_selected_path(file.path.to_string_lossy().into_owned().into());
                view.set_selected_detail(
                    format!(
                        "{} file size · {} on disk",
                        crate::system_ui::bytes(file.bytes),
                        crate::system_ui::bytes(file.allocated)
                    )
                    .into(),
                );
                let risk = risk::assess(&file.path);
                view.set_risk_level(risk.level);
                view.set_risk_label(risk.label.into());
                view.set_risk_reason(if verified_cache {
                    "Verified OpenCode package download. OpenCode can download it again if needed."
                } else { risk.reason }.into());
                view.set_selected_trashed(false);
                view.set_message("".into());
                view.set_confirming(false);
                view.set_selected_open(true);
                *selected.borrow_mut() = Some((file, verified_cache));
            });
    }
    {
        let (weak, worker, trash) = (ui.as_weak(), worker.clone(), trash_panel.clone());
        ui.global::<StorageView>().on_delete_file(move || {
            let Some(ui) = weak.upgrade() else { return };
            let view = ui.global::<StorageView>();
            if !view.get_selected_open()
                || view.get_cache_open()
                || view.get_busy()
            {
                return;
            }
            if view.get_selected_trashed() {
                trash.act(&ui, &worker, true);
                return;
            }
            if let Some((file, verified_cache)) = selected.borrow_mut().take() {
                view.set_busy(true);
                if worker.delete_tile(file, verified_cache).is_err() {
                    view.set_busy(false);
                    view.set_message("Could not move this file to Trash".into());
                }
            }
        });
    }
    {
        let (weak, state, worker) = (ui.as_weak(), catalog.clone(), worker.clone());
        ui.global::<StorageView>().on_select_category(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(category) = category::Category::from_index(index) else {
                return;
            };
            let view = ui.global::<StorageView>();
            if view.get_selected_open() {
                return;
            }
            let mut state = state.borrow_mut();
            state.select_category(category);
            view.set_category(index);
            if category == category::Category::Cache && !state.cache_loading {
                check_cache_tiles(&mut state, &worker);
            } else if category == category::Category::Downloads {
                if let Ok(generation) = worker.refresh() {
                    state.begin_refresh(generation);
                }
            } else if category == category::Category::Trash {
                trash_panel::TrashPanel::refresh(&ui, &worker);
            }
            state.publish(&ui);
        });
    }
    let weak = ui.as_weak();
    {
        let (weak, state, worker) = (ui.as_weak(), catalog.clone(), worker.clone());
        ui.global::<StorageView>().on_refresh_files(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.global::<StorageView>().get_busy() {
                return;
            }
            let mut state = state.borrow_mut();
            if state.showing_trash() {
                trash_panel::TrashPanel::refresh(&ui, &worker);
                return;
            }
            if state.showing_cache() && !state.cache_loading {
                check_cache_tiles(&mut state, &worker);
            }
            if let Ok(generation) = worker.refresh() {
                state.begin_refresh(generation);
            }
            state.publish(&ui);
        });
    }
    let timer_worker = worker.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            for update in updates.try_iter() {
                let view = ui.global::<StorageView>();
                match update {
                    worker::Update::Snapshot(generation, inventories) => {
                        let mut state = catalog.borrow_mut();
                        if state.snapshot(generation, inventories) {
                            state.publish(&ui);
                        }
                    }
                    worker::Update::Deleted(file, result) => {
                        view.set_busy(false);
                        view.set_confirming(false);
                        match result {
                            Ok(()) => {
                                catalog.borrow_mut().remove(&file);
                                catalog.borrow().publish(&ui);
                                view.set_selected_open(false);
                                view.set_message("".into());
                                ui.invoke_focus_desktop();
                            }
                            Err(error) => {
                                if catalog.borrow().showing_cache() {
                                    let mut state = catalog.borrow_mut();
                                    check_cache_tiles(&mut state, &timer_worker);
                                    state.publish(&ui);
                                }
                                view.set_message(format!("Could not move to Trash: {error}").into())
                            }
                        }
                    }
                    worker::Update::TrashEmptied(result) => {
                        view.set_busy(false);
                        view.set_empty_confirming(false);
                        trash_panel::TrashPanel::refresh(&ui, &timer_worker);
                        if let Err(error) = result { view.set_trash_message(error.into()); }
                    }
                    worker::Update::TrashListed(result) => trash_panel.listed(&ui, result),
                    worker::Update::TrashActed(entry, permanent, result) => {
                        if result.is_ok() && !permanent {
                            let mut state = catalog.borrow_mut();
                            if let Ok(generation) = timer_worker.refresh() {
                                state.restore(&entry.stored.original, generation);
                            }
                        }
                        trash_panel::TrashPanel::refresh(&ui, &timer_worker);
                        if view.get_selected_open() {
                            trash_panel::TrashPanel::acted(&ui, result);
                        } else {
                            view.set_busy(false);
                            view.set_trash_message(match result {
                                Ok(()) => "Item restored.".into(),
                                Err(error) => error.into(),
                            });
                        }
                    }
                    worker::Update::CacheTiles(result) => {
                        let mut state = catalog.borrow_mut();
                        state.finish_cache_check(result);
                        state.publish(&ui);
                    }
                    worker::Update::CacheChecked(result) => cache_dialog.checked(&ui, result),
                    worker::Update::CacheCleared(result) => {
                        catalog.borrow_mut().remove_all(&result.removed);
                        catalog.borrow().publish(&ui);
                        cache_dialog::cleared(&ui, &result);
                    }
                }
            }
        },
    );
    Watch {
        worker,
        _timer: timer,
    }
}

fn check_cache_tiles(state: &mut view::State, worker: &worker::Worker) {
    state.start_cache_check();
    if !worker.list_cache() {
        state.finish_cache_check(Err("Cache worker unavailable".into()));
    }
}
