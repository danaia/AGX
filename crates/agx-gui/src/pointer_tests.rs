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
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
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
    click(1150.0, 115.0);
    assert!(!ui.get_session_open());
    assert_eq!(*selected.borrow(), vec![1], "second row enters the TUI");
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
