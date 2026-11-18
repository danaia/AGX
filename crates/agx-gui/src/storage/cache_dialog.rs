//! Cache preview state; a confirmation consumes exactly the plan the user saw.
use super::{cache, worker::Worker};
use crate::{AppWindow, StorageView, system_ui::bytes};
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
pub(super) struct Dialog(Rc<RefCell<Option<cache::Plan>>>);

pub(super) fn install(ui: &AppWindow, worker: Worker) -> Dialog {
    let dialog = Dialog::default();
    let view = ui.global::<StorageView>();
    let (weak, checker) = (ui.as_weak(), worker.clone());
    view.on_check_cache(move || {
        let Some(ui) = weak.upgrade() else { return };
        let view = ui.global::<StorageView>();
        if view.get_selected_open() {
            return;
        }
        view.set_cache_open(true);
        view.set_selected_open(true);
        view.set_cache_ready(false);
        view.set_message("Checking OpenCode package downloads…".into());
        view.set_busy(true);
        if !checker.check_cache() {
            unavailable(&view);
        }
    });
    let (weak, pending) = (ui.as_weak(), dialog.clone());
    view.on_clear_cache(move || {
        let Some(ui) = weak.upgrade() else { return };
        let view = ui.global::<StorageView>();
        if !view.get_cache_open() || !view.get_cache_ready() || view.get_busy() {
            return;
        }
        let Some(plan) = pending.0.borrow_mut().take() else {
            return;
        };
        view.set_cache_ready(false);
        view.set_message("Clearing verified downloads…".into());
        view.set_busy(true);
        if !worker.clear_cache(plan) {
            unavailable(&view);
        }
    });
    let (weak, pending) = (ui.as_weak(), dialog.clone());
    view.on_close_cache(move || {
        let Some(ui) = weak.upgrade() else { return };
        let view = ui.global::<StorageView>();
        if view.get_busy() || !view.get_cache_open() {
            return;
        }
        pending.0.borrow_mut().take();
        view.set_selected_open(false);
        view.set_cache_open(false);
        view.set_cache_ready(false);
    });
    dialog
}

impl Dialog {
    pub fn checked(&self, ui: &AppWindow, result: Result<cache::Plan, String>) {
        let view = ui.global::<StorageView>();
        view.set_busy(false);
        match result {
            Ok(plan) => {
                let count = plan.count();
                let mut message = if count == 0 {
                    "No downloads passed the cleanup check.".into()
                } else {
                    format!(
                        "{count} verified download(s) · {} on disk",
                        bytes(plan.bytes())
                    )
                };
                if plan.skipped > 0 {
                    message += &format!(
                        "\n{} entries did not pass the check and will be left in place.",
                        plan.skipped
                    );
                }
                view.set_message(message.into());
                view.set_cache_ready(count > 0);
                if count > 0 {
                    *self.0.borrow_mut() = Some(plan);
                }
            }
            Err(error) => view.set_message(
                format!("Could not check the cache. No files were removed.\n{error}").into(),
            ),
        }
    }
}

pub(super) fn cleared(ui: &AppWindow, result: &cache::Outcome) {
    let view = ui.global::<StorageView>();
    let mut message = format!(
        "Cleared {} download(s) · {} on disk.",
        result.removed.len(),
        bytes(result.removed.iter().map(|file| file.allocated).sum())
    );
    if let Some(error) = &result.first_error {
        message += &format!(
            "\n{} could not be removed. First issue: {error}",
            result.failed
        );
    }
    view.set_message(message.into());
    view.set_busy(false);
}

fn unavailable(view: &StorageView<'_>) {
    view.set_busy(false);
    view.set_message("The cache worker is unavailable. No files were removed.".into());
}
