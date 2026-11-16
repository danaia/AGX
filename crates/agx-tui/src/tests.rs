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
        for page in 0..8 {
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
                assert_eq!(app.tabs.len(), 8);
                assert!(text.contains("q quit"), "Quit shortcut must remain visible");
            }
        }
    }
}
