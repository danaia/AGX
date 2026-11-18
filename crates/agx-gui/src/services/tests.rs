//! Run on the one shared headless desktop in pointer_tests.
use crate::{AppWindow, ServiceRow, Services};
use slint::{
    ComponentHandle,
    platform::{Key, WindowEvent},
};
use std::{cell::RefCell, rc::Rc};

pub(crate) fn review(ui: &AppWindow, render: &impl Fn()) {
    let requests = Rc::new(RefCell::new(Vec::<String>::new()));
    let received = requests.clone();
    ui.global::<Services>()
        .on_update(move |key| received.borrow_mut().push(key.to_string()));
    ui.global::<Services>().set_busy(false);
    let key = |text: slint::SharedString| {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    };
    let show = |pending: ServiceRow| {
        ui.global::<Services>().set_pending(pending);
        ui.global::<Services>().set_review_open(true);
        render();
    };
    show(ServiceRow {
        key: "kernel".into(),
        name: "Linux kernel".into(),
        restart_kind: "computer".into(),
        restart_label: "Reboot after update".into(),
        restart_detail: "Save your work. Reboot after installation.".into(),
        ..Default::default()
    });
    // An unavailable installer cannot be enabled by keyboard navigation.
    key(Key::Tab.into());
    key(Key::Return.into());
    assert!(!ui.global::<Services>().get_review_open());
    assert!(requests.borrow().is_empty());
    let desktop = ServiceRow {
        key: "desktop".into(),
        name: "ANGI desktop".into(),
        action: "Update".into(),
        restart_kind: "desktop".into(),
        restart_label: "Desktop restart".into(),
        restart_detail: "Closes terminal and browser sessions.".into(),
        ..Default::default()
    };
    show(desktop.clone());
    key(Key::Return.into());
    assert!(requests.borrow().is_empty(), "Enter defaults to Cancel");
    show(desktop.clone());
    key(Key::Tab.into());
    key(Key::Return.into());
    assert_eq!(&*requests.borrow(), &["desktop"]);
    assert!(!ui.global::<Services>().get_review_open());
    show(ServiceRow {
        restart_pending: true,
        ..desktop.clone()
    });
    key(Key::Tab.into());
    key(Key::Return.into());
    assert_eq!(
        requests.borrow().len(),
        1,
        "a pending restart cannot reinstall"
    );
    show(desktop);
    key(Key::Escape.into());
    assert!(!ui.global::<Services>().get_review_open());
    assert_eq!(requests.borrow().len(), 1);
    super::progress::apply(
        ui,
        &serde_json::json!({"state":"running", "phase":"building",
        "message":"Building desktop", "step":1, "total":7, "indeterminate":true, "log":"Compiling desktop"}),
    );
    render();
    assert!(ui.global::<Services>().get_progress_open());
    assert!(ui.global::<Services>().get_operation_indeterminate());
    key(Key::Escape.into());
    assert!(!ui.global::<Services>().get_progress_open());
    assert_eq!(
        ui.global::<Services>().get_operation_state(),
        "running",
        "Hide does not cancel the install"
    );
    super::progress::apply(
        ui,
        &serde_json::json!({"state":"succeeded", "phase":"complete",
        "message":"Installation verified", "release":"agx-gui.verified", "step":7, "total":7}),
    );
    render();
    assert!(
        ui.global::<Services>().get_progress_open(),
        "Completion is surfaced after hiding progress"
    );
    assert_eq!(ui.global::<Services>().get_operation_step(), 7);
    assert_eq!(
        ui.global::<Services>().get_operation_release(),
        "agx-gui.verified"
    );
    key(Key::Return.into());
    assert!(!ui.global::<Services>().get_progress_open());

    let services = ui.global::<Services>();
    // A new session (including after a reboot) starts without an active operation.
    services.set_operation_state("".into());
    for state in ["succeeded", "failed"] {
        super::progress::apply_receipt(ui, &serde_json::json!({"state": state}));
        assert!(services.get_operation_state().is_empty());
        assert!(!services.get_progress_open());
    }
    super::progress::apply_receipt(ui, &serde_json::json!({"state": "running"}));
    assert!(services.get_progress_open());
    key(Key::Escape.into());
    super::progress::apply_receipt(ui, &serde_json::json!({"state": "succeeded"}));
    assert!(
        services.get_progress_open(),
        "a resumed installer still reports completion"
    );
    key(Key::Return.into());
    super::progress::apply_receipt(ui, &serde_json::json!({"state": "succeeded"}));
    assert!(
        !services.get_progress_open(),
        "the next inventory check leaves the receipt closed"
    );
}
