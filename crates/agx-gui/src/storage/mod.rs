//! Storage inventory lifecycle and the heat map's Slint presentation model.
mod delete;
mod policy;
mod risk;
mod scan;
#[cfg(test)]
mod tests;
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
    let catalog = Rc::new(RefCell::new(view::State::default()));
    catalog.borrow().bind(ui);
    let selected = Rc::new(RefCell::new(None::<scan::File>));
    {
        let (weak, catalog, selected) = (ui.as_weak(), catalog.clone(), selected.clone());
        ui.global::<StorageView>()
            .on_select_file(move |drive, index| {
                let Some(ui) = weak.upgrade() else { return };
                let view = ui.global::<StorageView>();
                if view.get_busy() {
                    return;
                }
                let Some(file) = catalog
                    .borrow()
                    .files
                    .get(drive as usize)
                    .and_then(|files| files.get(index as usize))
                    .cloned()
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
                view.set_risk_reason(risk.reason.into());
                view.set_message("".into());
                view.set_confirming(false);
                view.set_selected_open(true);
                *selected.borrow_mut() = Some(file);
            });
    }
    {
        let (weak, worker) = (ui.as_weak(), worker.clone());
        ui.global::<StorageView>().on_delete_file(move || {
            let Some(ui) = weak.upgrade() else { return };
            let view = ui.global::<StorageView>();
            if !view.get_selected_open() || !view.get_confirming() || view.get_busy() {
                return;
            }
            if let Some(file) = selected.borrow_mut().take() {
                view.set_busy(true);
                if worker.delete(file).is_err() {
                    view.set_busy(false);
                    view.set_message("Could not start deletion".into());
                }
            }
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
            if let Ok(generation) = worker.refresh() {
                state.borrow_mut().begin_refresh(generation);
                state.borrow().publish(&ui);
            }
        });
    }
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
                                view.set_message(format!("Could not delete: {error}").into())
                            }
                        }
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
