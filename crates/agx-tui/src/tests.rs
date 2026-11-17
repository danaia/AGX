//! Layout and navigation regressions shared across screen modules.

use crate::{ColorMode, Options, app::App};
use agx_core::Sampler;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn gui_return_is_visible_in_profile_and_beside_power_actions() {
    let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
    let mut app = App::new(Options::default());
    app.gui_available = true;
    app.profile.gui_available = true;
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Back to GUI  F8"));
    assert!(text.contains("[GUI F8]"));
    assert!(text.contains("Shutdown F9"));
    assert!(text.contains("Reboot F10"));
    assert!(app.gui_footer.width > 0);
    assert!(app.profile.gui_button.height > 0);
}
#[test]
fn small_and_resized_layouts_keep_help_and_navigation_available() {
    let options = Options {
        ascii: true,
        color: ColorMode::Never,
        mouse: false,
    };
    for (width, height) in [(20, 6), (40, 12), (80, 24), (120, 36)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut app = App::new(options);
        app.snapshot = Some(Sampler::default().collect().unwrap());
        for page in 0..9 {
            app.page(page);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains("ANGI") || text.contains("Overview"));
            assert!(text.is_ascii(), "ASCII mode emitted Unicode");
            if width >= 40 {
                assert_eq!(app.tabs.len(), 9);
                assert!(text.contains("q quit"), "Quit shortcut must remain visible");
            }
        }
    }
}

#[test]
fn network_and_services_show_vnc_connections_and_gui_memory() {
    for (width, height) in [(80, 24), (120, 36)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut app = App::new(Options {
            ascii: true,
            ..Options::default()
        });
        let mut snapshot = Sampler::default().collect().unwrap();
        snapshot.vnc = agx_core::VncStatus {
            port: 5900,
            listeners: vec!["127.0.0.1:5900".into()],
            connections: 2,
            gui_rss_bytes: Some(64 * 1048576),
            error: None,
        };
        app.snapshot = Some(snapshot);
        for page in [3, 5] {
            app.page(page);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains("127.0.0.1:5900"));
            assert!(text.contains("2 connection(s)"));
            assert!(text.contains("64.0 MiB RSS"));
        }
        app.snapshot.as_mut().unwrap().vnc.listeners.clear();
        app.page(3);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("not listening"));
    }
}

#[test]
fn terminal_screen_explains_shell_return_and_persistent_sessions() {
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    let mut app = App::new(Options {
        ascii: true,
        ..Options::default()
    });
    app.page(7);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Open shell"));
    assert!(text.contains("Ctrl+D"));
    assert!(text.contains("tmux"));
    assert!(text.contains("[Shell F4]"));
    assert!(app.shell_button.height > 0 && app.tmux_button.height > 0);
    app.page(8);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("F4"));
    assert!(text.contains("HEADLESS BY DESIGN"));
}
