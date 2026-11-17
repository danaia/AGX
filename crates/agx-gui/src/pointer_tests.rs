use super::*;

#[test]
fn session_dropdown_routes_only_the_four_session_actions() {
    assert_eq!(session_action(0), Some(Cmd::Logout));
    assert_eq!(session_action(1), Some(Cmd::Tui));
    assert_eq!(session_action(2), Some(Cmd::Reboot));
    assert_eq!(session_action(3), Some(Cmd::Shutdown));
    assert_eq!(session_action(-1), None);
    assert_eq!(session_action(4), None);
}

#[test]
fn session_button_opens_dropdown_and_mouse_selects_tui() {
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{
        Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent,
    };
    struct TestPlatform(Rc<MinimalSoftwareWindow>);
    impl Platform for TestPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.0.clone())
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.window().set_size(slint::PhysicalSize::new(1280, 720));
    ui.show().unwrap();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let result = selected.clone();
    ui.on_session_action(move |index| result.borrow_mut().push(index));
    let render = || {
        window.draw_if_needed(|renderer| {
            renderer.render(&mut vec![slint::Rgb8Pixel::default(); 1280 * 720], 1280);
        });
    };
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    };
    render();
    click(1246.0, 17.0);
    assert!(
        ui.get_session_open(),
        "header button must open the dropdown"
    );
    ui.set_draw_cursor(true);
    render();
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(1150.0, 115.0),
    });
    assert_eq!(ui.get_cursor_x(), 1150.0);
    assert_eq!(ui.get_cursor_y(), 115.0);
    click(1150.0, 155.0);
    assert!(!ui.get_session_open());
    assert_eq!(*selected.borrow(), vec![1], "third row enters the TUI");
    render();
    click(1246.0, 17.0);
    assert!(ui.get_session_open());
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    assert!(
        !ui.get_session_open(),
        "Escape dismisses without choosing an action"
    );
    assert_eq!(*selected.borrow(), vec![1]);
    render();
    click(1246.0, 17.0);
    render();
    {
        let mut terminals = App::new();
        terminals.run(&ui, Cmd::Open(Kind::Terminal));
        let first = terminals.desk.focused().unwrap();
        ui.global::<Terminal>().set_font_size(19.);
        for _ in 0..11 { terminals.run(&ui, Cmd::Open(Kind::Terminal)); }
        assert_eq!(terminals.desk.scene().windows.len(), 16);
        assert_ne!(terminals.desk.focused(), Some(first));
        assert_eq!(ui.global::<Terminal>().get_font_size(), 14.);
        terminals.terminal_select(&ui, first);
        assert_eq!(ui.global::<Terminal>().get_font_size(), 19.);
        ui.global::<Terminal>().set_font_size(14.);
    }
    let settings_app = Rc::new(RefCell::new(App::new()));
    ui.set_wins(ModelRc::from(settings_app.borrow().view.windows.clone()));
    {
        let (weak, state) = (ui.as_weak(), settings_app.clone());
        ui.on_settings_open(move || {
            state
                .borrow_mut()
                .run(&weak.upgrade().unwrap(), Cmd::Open(Kind::Settings));
        });
        let (weak, state) = (ui.as_weak(), settings_app.clone());
        ui.on_pointer(move |kind, button, x, y, ctrl, alt, shift, meta| {
            state.borrow_mut().pointer(
                &weak.upgrade().unwrap(),
                kind,
                button,
                x,
                y,
                mods(ctrl, alt, shift, meta),
            );
        });
    }
    click(1150.0, 70.0);
    assert!(
        settings_app.borrow().desk.is_open(Kind::Settings),
        "Settings opens as a tile from the first menu row"
    );
    render();
    if let Some(path) = std::env::var_os("AGX_SETTINGS_SCREENSHOT") {
        use std::io::Write;
        let mut pixels = vec![slint::Rgb8Pixel::default(); 1280 * 720];
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, 1280);
        });
        let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        file.write_all(b"P6\n1280 720\n255\n").unwrap();
        for p in pixels {
            file.write_all(&[p.r, p.g, p.b]).unwrap();
        }
    }
    {
        let (weak, state) = (ui.as_weak(), settings_app.clone());
        ui.global::<MouseSettings>().on_refresh(move || {
            state
                .borrow_mut()
                .run(&weak.upgrade().unwrap(), Cmd::RefreshDesktop);
        });
        let (weak, state) = (ui.as_weak(), settings_app.clone());
        ui.on_confirm(move |yes| state.borrow_mut().confirm(&weak.upgrade().unwrap(), yes));
    }
    let tile = settings_app.borrow().desk.rect_of(Kind::Settings).unwrap();
    click(
        if tile.w - 12. < 500. {
            tile.x + 98.
        } else {
            tile.right() - 98.
        },
        tile.bottom() - 34.,
    );
    assert!(ui.get_confirm_text().contains("stay signed in"));
    assert_eq!(settings_app.borrow().pending, Some(Cmd::RefreshDesktop));
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    assert!(ui.get_confirm_text().is_empty());
    assert_eq!(settings_app.borrow().pending, None);
    assert!(settings_app.borrow().desk.is_open(Kind::Settings));
    // Tab stays within the Settings controls, and the speed slider accepts arrow keys.
    for _ in 0..1 {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: slint::platform::Key::Tab.into(),
        });
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::RightArrow.into(),
    });
    let first_speed = ui.global::<MouseSettings>().get_speed();
    assert!(first_speed > 0., "keyboard changes pointer speed");
    for _ in 0..7 {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: slint::platform::Key::Tab.into(),
        });
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::RightArrow.into(),
    });
    assert!(
        ui.global::<MouseSettings>().get_speed() > first_speed,
        "Tab wraps within Settings"
    );
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    assert!(
        settings_app.borrow().desk.is_open(Kind::Settings),
        "Escape keeps the tile open"
    );
    let settings_rect = settings_app.borrow().desk.rect_of(Kind::Settings).unwrap();
    click(settings_rect.right() - 12., settings_rect.y + 14.);
    assert!(
        !settings_app.borrow().desk.is_open(Kind::Settings),
        "normal window close removes Settings"
    );
    assert_eq!(
        *selected.borrow(),
        vec![1],
        "Settings never triggers a session action"
    );
    render();
    let wheel = Rc::new(RefCell::new(Vec::new()));
    let received = wheel.clone();
    ui.on_scroll(move |dx, dy, _, _, _, _| received.borrow_mut().push((dx, dy)));
    remote::input::dispatch(
        ui.window(),
        remote::input::Input::Pointer {
            x: 600,
            y: 500,
            action: remote::input::PointerAction::Scroll { dx: 1, dy: 1 },
        },
    );
    assert_eq!(
        *wheel.borrow(),
        vec![(-60.0, 60.0)],
        "RFB right/up wheels reach the desktop in Slint's content-motion coordinates"
    );
    let mut app = App::new();
    ui.set_wins(ModelRc::from(app.view.windows.clone()));
    app.desk.open(Kind::Web);
    app.refresh(&ui);
    render();
    let keys = Rc::new(RefCell::new(Vec::new()));
    let received = keys.clone();
    ui.on_key(move |name, _, _, _, meta| {
        received.borrow_mut().push((name.to_string(), meta));
        true
    });
    app.desk.close(Kind::Web);
    app.refresh(&ui);
    render();
    remote::input::dispatch(
        ui.window(),
        remote::input::Input::Key {
            down: true,
            keysym: 0xffeb,
        },
    );
    remote::input::dispatch(
        ui.window(),
        remote::input::Input::Key {
            down: true,
            keysym: 32,
        },
    );
    assert!(
        keys.borrow()
            .iter()
            .any(|(name, meta)| name == "space" && *meta),
        "closing the focused browser restores desktop Super shortcuts"
    );
    for keysym in [32, 0xffeb] {
        remote::input::dispatch(ui.window(), remote::input::Input::Key { down: false, keysym });
    }
    // A map tile opens details. Deletion needs a second activation, and Escape
    // cancels confirmation without invoking the deletion callback.
    let storage = ui.global::<StorageView>();
    storage.set_sd_cells(ModelRc::new(slint::VecModel::from(vec![StorageCell {
        path: "/home/test/Downloads/archive.zip".into(),
        detail: "32 MiB file size".into(),
        tint: slint::Color::from_rgb_u8(60, 110, 70),
    }])));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let received = selected.clone();
    let weak = ui.as_weak();
    storage.on_select_file(move |drive, index| {
        received.borrow_mut().push((drive, index));
        let ui = weak.upgrade().unwrap();
        let storage = ui.global::<StorageView>();
        storage.set_selected_path("/home/test/Downloads/archive.zip".into());
        storage.set_selected_detail("32 MiB file size".into());
        storage.set_selected_open(true);
    });
    let deletions = Rc::new(RefCell::new(0));
    let received = deletions.clone();
    storage.on_delete_file(move || *received.borrow_mut() += 1);
    render();
    let overview = app.desk.rect_of(Kind::Overview).unwrap();
    // Use the actual model's desktop handler so a premature focus/stack update
    // would destroy the pressed tile and lose the first click.
    let live_app = Rc::new(RefCell::new(app));
    let (weak, state) = (ui.as_weak(), live_app.clone());
    ui.on_pointer(move |kind, button, x, y, ctrl, alt, shift, meta| {
        state.borrow_mut().pointer(&weak.upgrade().unwrap(), kind, button, x, y, mods(ctrl, alt, shift, meta));
    });
    click(overview.x + 25., overview.y + 200.);
    assert_eq!(*selected.borrow(), vec![(0, 0)]);
    assert!(storage.get_selected_open());
    render();
    let key = |text: slint::SharedString| {
        ui.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        ui.window().dispatch_event(WindowEvent::KeyReleased { text });
    };
    key(slint::platform::Key::Tab.into());
    key(slint::platform::Key::Return.into());
    assert!(storage.get_confirming());
    assert_eq!(*deletions.borrow(), 0);
    key(slint::platform::Key::Escape.into());
    assert!(!storage.get_confirming());
    assert_eq!(*deletions.borrow(), 0);
    key(slint::platform::Key::Tab.into());
    key(slint::platform::Key::Return.into());
    assert!(storage.get_confirming());
    key(slint::platform::Key::Tab.into());
    key(slint::platform::Key::Return.into());
    assert_eq!(*deletions.borrow(), 1);
    storage.set_selected_open(false);
}

#[test]
fn floating_edges_and_corners_select_directional_resize_cursors() {
    let mut app = App::new();
    app.desk.toggle_float();
    app.view.sync(app.desk.scene(), None);
    let r = app.desk.rect_of(app.desk.focused().unwrap()).unwrap();
    for (x, y, cursor) in [
        (r.x, r.y + r.h / 2.0, 2),
        (r.right() - 1.0, r.y + r.h / 2.0, 2),
        (r.x + r.w / 2.0, r.y, 3),
        (r.x + r.w / 2.0, r.bottom() - 1.0, 3),
        (r.x, r.y, 4),
        (r.right() - 1.0, r.bottom() - 1.0, 4),
        (r.right() - 1.0, r.y, 5),
        (r.x, r.bottom() - 1.0, 5),
    ] {
        assert_eq!(app.hover(x, y), cursor, "cursor at {x},{y}");
    }
}

#[test]
fn hover_uses_cached_scene_and_never_requests_model_updates() {
    let mut app = App::new();
    let before: Vec<_> = app.view.windows.iter().collect();
    let windows = app.view.scene.borrow().windows.as_ptr();
    for i in 0..10_000 {
        assert_eq!(
            app.moved((i % 1280) as f32, (i % 720) as f32),
            Motion::Cursor
        );
    }
    assert_eq!(app.view.scene.borrow().windows.as_ptr(), windows);
    assert_eq!(app.view.windows.iter().collect::<Vec<_>>(), before);
}

#[test]
fn tiled_drag_is_overlay_only_and_drop_commits_the_swap() {
    let mut app = App::new();
    app.desk.cycle_layout();
    app.view.sync(app.desk.scene(), None);
    let source = app.view.scene.borrow().rect_of(Kind::Overview).unwrap();
    let target = app.view.scene.borrow().rect_of(Kind::Cpu).unwrap();
    app.begin_move(Kind::Overview, source.x + 40.0, source.y + 10.0);
    let (x, y) = target.center();
    assert_eq!(app.moved(x, y), Motion::Overlay);
    assert_eq!(app.target, Some(Kind::Cpu));
    assert_eq!(app.desk.rect_of(Kind::Overview), Some(source));
    app.up(x, y, true);
    assert_eq!(app.desk.rect_of(Kind::Overview), Some(target));
    assert!(app.ghost.is_none());
    assert!(app.target.is_none());
}
