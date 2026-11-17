//! AGX desktop: a Slint front end over the same system data as the TUI, with
//! Omarchy/Hyprland-style window management (see wm.rs and keymap.rs).
//!
//! Exit codes let a launcher script react to the user's choice:
//! 0 = quit, 10 = the user asked for the TUI instead.
mod keymap;
mod browser;
mod input;
mod launcher;
mod wm;
mod scene_model;
mod login;
mod graphics;
mod remote;

use agx_core::{Sampler, Snapshot};
use keymap::{Cmd, ModKey, Mods};
use input::Drag;
#[cfg(test)]
use input::Motion;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use scene_model::SceneModel;
use std::{
    cell::RefCell,
    fs,
    process::{Command, ExitCode},
    rc::Rc,
    sync::atomic::{AtomicI32, Ordering},
    time::Duration,
};
use wm::{Desktop, Full, Kind, Mode, Rect};

slint::include_modules!();

const LOGO: &str = include_str!("../../../brand/logo-ascii.md");
const EXIT_TUI: i32 = 10;
const TOP: f32 = 34.0;
const BOTTOM: f32 = 26.0;
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);


struct App {
    browser: browser::Browser,
    desk: Desktop,
    view: SceneModel,
    occupied: Rc<VecModel<bool>>,
    modk: ModKey,
    drag: Drag,
    mods: Mods,
    last: (f32, f32),
    target: Option<Kind>,
    ghost: Option<Rect>,
    cursor: i32,
    menu: Vec<Cmd>,
    pending: Option<Cmd>,
    /// Logical size of the window as last laid out.
    size: (f32, f32),
    header_clicks: input::HeaderClicks,
}


fn session_action(index: i32) -> Option<Cmd> {
    [Cmd::Logout, Cmd::Tui, Cmd::Reboot, Cmd::Shutdown].get(index as usize).copied()
}

impl App {
    fn new() -> Self {
        let screen = Rect::new(0.0, 0.0, 1280.0, 720.0);
        let desk = Desktop::new(screen, Rect::new(0.0, TOP, 1280.0, 720.0 - TOP - BOTTOM));
        let view = SceneModel::default();
        view.sync(desk.scene(), None);
        Self {
            browser: browser::Browser::default(),
            desk,
            view,
            occupied: Rc::new(VecModel::from(vec![false; wm::WORKSPACES])),
            modk: ModKey::from_env(),
            drag: Drag::None,
            mods: Mods::default(),
            last: (0.0, 0.0),
            target: None,
            ghost: None,
            cursor: 0,
            menu: vec![],
            pending: None,
            size: (1280.0, 720.0),
            header_clicks: input::HeaderClicks::default(),
        }
    }

    fn resized(&mut self, ui: &AppWindow, w: f32, h: f32) {
        if std::env::var_os("AGX_GUI_DEBUG").is_some() {
            eprintln!("layout area {w}x{h}");
        }
        self.size = (w, h);
        ui.global::<Sys>().set_screen_size(format!("{}x{}", w as i32, h as i32).into());
        self.desk.set_area(
            Rect::new(0.0, 0.0, w, h),
            Rect::new(0.0, TOP, w, (h - TOP - BOTTOM).max(0.0)),
        );
        self.refresh(ui);
    }

    fn refresh(&self, ui: &AppWindow) {
        self.view.sync(self.desk.scene(), self.target);
        let browser_visible = self.view.scene.borrow().windows.iter()
            .any(|window| window.kind == Kind::Web && !window.collapsed);
        if self.desk.focused() != Some(Kind::Web) || !browser_visible {
            ui.invoke_focus_desktop();
        }
        for i in 0..wm::WORKSPACES {
            let occupied = self.desk.occupied(i);
            if self.occupied.row_data(i) != Some(occupied) {
                self.occupied.set_row_data(i, occupied);
            }
        }
        ui.set_workspace(self.desk.current() as i32);
        ui.set_windowed_mode(self.desk.mode() == Mode::Floating);
        ui.set_layout_name(
            match (self.desk.fullscreen_of(), self.desk.mode()) {
                (Some((_, Full::Screen)), _) => "fullscreen",
                (Some((_, Full::Maximized)), _) => "maximized",
                (None, Mode::Tiled) => "tiled",
                (None, Mode::Floating) => "windowed",
            }
            .into(),
        );
        ui.set_focus_title(self.desk.focused().map_or("", Kind::title).into());
        ui.set_screen_full(self.view.scene.borrow().screen_full);
        self.refresh_drag(ui);
    }

    fn refresh_drag(&self, ui: &AppWindow) {
        ui.set_dragging(!matches!(self.drag, Drag::None));
        ui.set_cursor(self.cursor);
        match self.ghost {
            Some(g) => {
                ui.set_ghost_x(g.x);
                ui.set_ghost_y(g.y);
                ui.set_ghost_w(g.w);
                ui.set_ghost_h(g.h);
                ui.set_ghost_visible(true);
            }
            None => ui.set_ghost_visible(false),
        }
    }

    fn toast(&self, ui: &AppWindow, text: &str) {
        ui.set_toast(text.into());
        let weak = ui.as_weak();
        Timer::single_shot(Duration::from_secs(4), move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_toast("".into());
            }
        });
    }

    // --- commands -----------------------------------------------------------
    fn run(&mut self, ui: &AppWindow, cmd: Cmd) {
        self.header_clicks = input::HeaderClicks::default();
        let m = self.modk.name();
        match cmd {
            Cmd::Focus(d) => self.desk.focus_dir(d),
            Cmd::Swap(d) => self.desk.swap_dir(d),
            Cmd::Close => {
                if let Some(k) = self.desk.focused() {
                    if k == Kind::Web && self.browser_request_close() { return; }
                    self.desk.close(k);
                    self.toast(ui, &format!("Closed {}. Reopen it from Apps ({m}+Space).", k.title()));
                }
            }
            Cmd::ToggleSplit => self.desk.toggle_split(),
            Cmd::ToggleFloat => {
                if let Some(note) = self.desk.toggle_float() {
                    self.toast(ui, note);
                }
            }
            Cmd::Fullscreen => self.desk.toggle_fullscreen(Full::Screen),
            Cmd::Maximize => self.desk.toggle_fullscreen(Full::Maximized),
            Cmd::CycleLayout => self.desk.cycle_layout(),
            Cmd::Workspace(n) => self.desk.goto(n),
            Cmd::MoveToWorkspace { ws, follow } => self.desk.move_to_workspace(ws, follow),
            Cmd::NextWorkspace => self.desk.next_workspace(false),
            Cmd::PrevWorkspace => self.desk.next_workspace(true),
            Cmd::CycleFocus(back) => self.desk.cycle_focus(back),
            Cmd::Resize { dw, dh } => self.desk.resize(dw, dh),
            Cmd::Menu => self.open_menu(ui),
            Cmd::Open(k) => self.desk.open(k),
            Cmd::Tui => quit(EXIT_TUI),
            Cmd::Logout => quit(0),
            Cmd::Reboot | Cmd::Shutdown => {
                ui.set_confirm_text(
                    if cmd == Cmd::Reboot {
                        "Reboot this Pi? Network sessions will close."
                    } else {
                        "Power off this Pi? Network sessions will close."
                    }
                    .into(),
                );
                self.pending = Some(cmd);
            }
        }
        self.refresh(ui);
    }

    fn confirm(&mut self, ui: &AppWindow, yes: bool) {
        ui.set_confirm_text("".into());
        let cmd = self.pending.take();
        if yes {
            power(ui, cmd);
        }
    }

}

fn mods(ctrl: bool, alt: bool, shift: bool, meta: bool) -> Mods {
    Mods { ctrl, alt, shift, meta }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--help") {
        println!("ANGI desktop\nUsage: agx-gui [--login | --login-preview | --cursor-preview | --prepare-web-storage]");
        return ExitCode::SUCCESS;
    }
    if std::env::args().nth(1).as_deref() == Some("--prepare-web-storage") {
        return match browser::prepare_accounts() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => { eprintln!("agx-gui: cannot prepare Web storage: {error}"); ExitCode::FAILURE }
        };
    }
    prepare_display();
    if let Err(error) = graphics::initialize() {
        eprintln!("agx-gui: {error}");
        return ExitCode::FAILURE;
    }
    quit_on_signals();
    let result = match std::env::args().nth(1).as_deref() {
        Some("--cursor-preview") => CursorPreviewWindow::new().map_err(Into::into).and_then(|ui| ui.run().map_err(Into::into)),
        Some("--login") => login::run(false),
        Some("--login-preview") => login::run(true),
        _ => run(),
    };
    // Keys typed into the GUI also reach the terminal; do not replay them into the shell.
    // SAFETY: tcflush on stdin is harmless when it is not a terminal.
    unsafe { libc::tcflush(0, libc::TCIFLUSH) };
    if let Err(error) = result {
        eprintln!("agx-gui: {error}");
        return ExitCode::from(1);
    }
    ExitCode::from(EXIT_CODE.load(Ordering::SeqCst) as u8)
}

/// The framebuffer backend puts the console in graphics mode and only restores it
/// when the program exits normally, so Ctrl+C or a kill must end the event loop
/// instead of the process.
fn quit_on_signals() {
    use signal_hook::{
        consts::{SIGHUP, SIGINT, SIGTERM},
        iterator::Signals,
    };
    if let Ok(mut signals) = Signals::new([SIGINT, SIGTERM, SIGHUP]) {
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = slint::invoke_from_event_loop(|| {
                    let _ = slint::quit_event_loop();
                });
            }
        });
    }
}

/// The appliance image has no udev, so nothing loads the display driver at
/// boot. Load it here when the DRM device is missing.
#[cfg(feature = "kms")]
fn prepare_display() {
    if unsafe { libc::geteuid() } == 0 {
        // vc4 drives HDMI; v3d provides the Pi 5's GPU render device.
        let _ = Command::new("modprobe").arg("v3d").status();
    }
    let card = std::path::Path::new("/dev/dri/card0");
    if card.exists() {
        return;
    }
    // HDMI reads the monitor over the SoC's I2C bus; without that driver vc4 never finishes binding.
    for module in ["i2c-brcmstb", "vc4"] {
        match Command::new("modprobe").arg(module).status() {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("agx-gui: modprobe {module} exited with {status}"),
            Err(error) => eprintln!("agx-gui: modprobe {module}: {error}"),
        }
    }
    for _ in 0..50 {
        if card.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("agx-gui: /dev/dri/card0 did not appear; is dtoverlay=vc4-kms-v3d set in config.txt?");
}
#[cfg(not(feature = "kms"))]
fn prepare_display() {}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let ui = AppWindow::new()?;
    graphics::attach_input(&ui);
    apply_omarchy_theme(&ui);
    ui.global::<Sys>().set_logo(LOGO.trim_end().into());
    ui.global::<Sys>().set_mode_note(
        if cfg!(feature = "kms") {
            "DRM/KMS appliance, no display server"
        } else {
            "desktop preview window; power actions disabled"
        }
        .into(),
    );

    let app = Rc::new(RefCell::new(App::new()));
    ui.set_wins(ModelRc::from(app.borrow().view.windows.clone()));
    ui.set_ws_occupied(ModelRc::from(app.borrow().occupied.clone()));
    ui.set_mod_name(app.borrow().modk.name().into());
    ui.set_draw_cursor(graphics::draw_cursor());
    ui.set_resize_cursor_overlay(cfg!(feature = "kms"));
    let _remote = remote::start(&ui);
    // Callbacks never nest, but a stray re-entrant event is dropped rather than panicking.
    macro_rules! hook {
        ($on:ident, |$app:ident, $ui:ident $(, $arg:ident)*| $body:expr) => {{
            let (weak, state) = (ui.as_weak(), app.clone());
            ui.$on(move |$($arg),*| {
                let (Some($ui), Ok(mut $app)) = (weak.upgrade(), state.try_borrow_mut()) else {
                    return Default::default();
                };
                $body
            });
        }};
    }
    hook!(on_key, |a, ui, name, ctrl, alt, shift, meta| {
        if std::env::var_os("AGX_GUI_DEBUG").is_some() {
            eprintln!("key {name:?} ctrl={ctrl} alt={alt} shift={shift} meta={meta}");
        }
        a.key(&ui, &name, mods(ctrl, alt, shift, meta))
    });
    hook!(on_pointer, |a, ui, kind, button, x, y, ctrl, alt, shift, meta| {
        a.pointer(&ui, kind, button, x, y, mods(ctrl, alt, shift, meta))
    });
    hook!(on_scroll, |a, ui, dx, delta, ctrl, alt, shift, meta| a.scroll(&ui, dx, delta, mods(ctrl, alt, shift, meta)));
    hook!(on_resized, |a, ui, w, h| a.resized(&ui, w, h));
    hook!(on_menu_query, |a, ui, query| a.set_menu_rows(&ui, &query));
    hook!(on_menu_accept, |a, ui, index| a.menu_accept(&ui, index));
    hook!(on_confirm, |a, ui, yes| a.confirm(&ui, yes));
    hook!(on_session_action, |a, ui, index| {
        if let Some(cmd) = session_action(index) { a.run(&ui, cmd); }
    });
    hook!(on_layout_toggle, |a, ui| a.run(&ui, Cmd::CycleLayout));
    let _browser_watch = browser::install(&ui, &app);
    app.borrow().refresh(&ui);

    // The compositor-less backend sizes the window after the UI is built, and a change
    // notification alone left the layout at its 1280x720 default on a 2560x1080 display.
    // Compare against the real window size and re-lay out whenever it differs.
    let size_watch = Timer::default();
    {
        let (weak, state) = (ui.as_weak(), app.clone());
        size_watch.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) else { return };
            let (size, scale) = (ui.window().size(), ui.window().scale_factor());
            let (w, h) = (size.width as f32 / scale, size.height as f32 / scale);
            if w > 0.0 && h > 0.0 && (w, h) != app.size {
                app.resized(&ui, w, h);
            }
        });
    }

    // Clock on the UI thread.
    let clock = Timer::default();
    {
        let weak = ui.as_weak();
        let tick = move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_clock(local_time().into());
            }
        };
        tick();
        clock.start(TimerMode::Repeated, Duration::from_secs(1), tick);
    }

    // Sampling stays off the UI thread; results are applied via the event loop.
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let mut sampler = Sampler::default();
        loop {
            let result = sampler.collect();
            let weak = weak.clone();
            if slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    match result {
                        Ok(snapshot) => apply(&ui, &snapshot),
                        Err(error) => ui.global::<Sys>().set_warnings(format!("sampling failed: {error}").into()),
                    }
                }
            })
            .is_err()
            {
                break;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });

    ui.run()?;
    Ok(())
}

fn quit(code: i32) {
    EXIT_CODE.store(code, Ordering::SeqCst);
    let _ = slint::quit_event_loop();
}

fn power(ui: &AppWindow, cmd: Option<Cmd>) {
    let verb = match cmd {
        Some(Cmd::Reboot) => "reboot",
        Some(Cmd::Shutdown) => "shutdown",
        _ => return,
    };
    let sys = ui.global::<Sys>();
    if cfg!(feature = "kms") {
        // The CLI already confirms by flag; this GUI asked the user first.
        match Command::new("agx").args([verb, "--yes"]).status() {
            Ok(status) if status.success() => {}
            Ok(status) => sys.set_warnings(format!("agx {verb} exited with {status}").into()),
            Err(error) => sys.set_warnings(format!("agx {verb}: {error}").into()),
        }
    } else {
        sys.set_warnings(format!("{verb} is disabled in the desktop preview").into());
    }
}

fn apply(ui: &AppWindow, s: &Snapshot) {
    let sys = ui.global::<Sys>();
    sys.set_hostname(s.hostname.clone().into());
    sys.set_board(s.board.clone().into());
    sys.set_kernel(s.kernel.clone().into());
    sys.set_uptime(duration(s.uptime_secs).into());
    sys.set_cpu_percent(s.cpu_usage_percent.unwrap_or(0.0) as f32);
    sys.set_cpu_detail(format!("{} cores", s.cpu_count).into());
    sys.set_load_text(
        s.load_average.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>().join("  ").into(),
    );
    let mem_used = s.memory.total_bytes.saturating_sub(s.memory.available_bytes);
    sys.set_mem_percent(percent(mem_used, s.memory.total_bytes));
    sys.set_mem_detail(format!("{} / {}", bytes(mem_used), bytes(s.memory.total_bytes)).into());
    match &s.root_storage {
        Some(d) => {
            let used = d.total_bytes.saturating_sub(d.available_bytes);
            sys.set_disk_percent(percent(used, d.total_bytes));
            sys.set_disk_detail(format!("{} / {}", bytes(used), bytes(d.total_bytes)).into());
        }
        None => sys.set_disk_detail("unavailable".into()),
    }
    let primary = s
        .networks
        .iter()
        .filter(|n| n.name != "lo")
        .find_map(|n| n.ipv4.first().cloned())
        .unwrap_or_else(|| "no network".into());
    sys.set_primary_ip(primary.into());
    let nets: Vec<NetRow> = s
        .networks
        .iter()
        .map(|n| NetRow {
            name: n.name.clone().into(),
            state: n.state.clone().into(),
            mac: n.mac.clone().into(),
            ipv4: if n.ipv4.is_empty() { "-".into() } else { n.ipv4.join(", ").into() },
        })
        .collect();
    sys.set_networks(ModelRc::new(VecModel::from(nets)));
    let hw: Vec<HwRow> = s
        .hardware
        .iter()
        .map(|h| HwRow {
            kind: h.kind.clone().into(),
            name: h.name.clone().into(),
            state: h.state.clone().into(),
            detail: h.detail.clone().into(),
        })
        .collect();
    sys.set_hardware(ModelRc::new(VecModel::from(hw)));
    sys.set_warnings(s.warnings.join("; ").into());
}

fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 * 100.0 / whole as f64) as f32
    }
}

fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m {}s", secs % 60)
    }
}

fn local_time() -> String {
    // SAFETY: localtime_r writes only into the zeroed tm we pass.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::new();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

/// Follow Omarchy's active theme when its palette file is present, so the
/// GUI matches a desktop it is previewed on. The appliance falls back to the
/// built-in Tokyo Night defaults.
fn apply_omarchy_theme(ui: &AppWindow) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let path = std::path::Path::new(&home).join(".config/omarchy/current/theme/colors.toml");
    let Ok(text) = fs::read_to_string(path) else { return };
    let color = |key: &str| -> Option<slint::Color> {
        let line = text.lines().find(|l| l.split('=').next().map(str::trim) == Some(key))?;
        let hex = line.split('=').nth(1)?.trim().trim_matches('"').trim_start_matches('#');
        let n = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6)?;
        Some(slint::Color::from_rgb_u8((n >> 16) as u8, (n >> 8) as u8, n as u8))
    };
    let t = ui.global::<Theme>();
    macro_rules! set {
        ($setter:ident, $key:expr) => {
            if let Some(c) = color($key) {
                t.$setter(c);
            }
        };
    }
    set!(set_bg, "background");
    set!(set_bg_dark, "dark_background");
    set!(set_bg_light, "lighter_background");
    set!(set_fg, "foreground");
    set!(set_fg_bright, "bright_foreground");
    set!(set_fg_dim, "dark_foreground");
    set!(set_muted, "muted");
    set!(set_accent, "accent");
    set!(set_green, "green");
    set!(set_yellow, "yellow");
    set!(set_red, "red");
    set!(set_cyan, "bright_cyan");
    set!(set_magenta, "bright_magenta");
}


#[cfg(test)]
mod pointer_tests {
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
        use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent, PointerEventButton};
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        struct TestPlatform(Rc<MinimalSoftwareWindow>);
        impl Platform for TestPlatform {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> { Ok(self.0.clone()) }
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
            ui.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
            ui.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
        };
        render();
        click(1246.0, 17.0);
        assert!(ui.get_session_open(), "header button must open the dropdown");
        ui.set_draw_cursor(true);
        render();
        ui.window().dispatch_event(WindowEvent::PointerMoved { position: slint::LogicalPosition::new(1150.0, 115.0) });
        assert_eq!(ui.get_cursor_x(), 1150.0);
        assert_eq!(ui.get_cursor_y(), 115.0);
        click(1150.0, 115.0);
        assert!(!ui.get_session_open());
        assert_eq!(*selected.borrow(), vec![1], "second row enters the TUI");
        render();
        click(1246.0, 17.0);
        assert!(ui.get_session_open());
        ui.window().dispatch_event(WindowEvent::KeyPressed { text: slint::platform::Key::Escape.into() });
        assert!(!ui.get_session_open(), "Escape dismisses without choosing an action");
        assert_eq!(*selected.borrow(), vec![1]);
        render();
        let wheel = Rc::new(RefCell::new(Vec::new()));
        let received = wheel.clone();
        ui.on_scroll(move |dx, dy, _, _, _, _| received.borrow_mut().push((dx, dy)));
        remote::input::dispatch(ui.window(), remote::input::Input::Pointer {
            x: 600, y: 500,
            action: remote::input::PointerAction::Scroll { dx: 1, dy: 1 },
        });
        assert_eq!(*wheel.borrow(), vec![(-60.0, 60.0)], "RFB right/up wheels reach the desktop in Slint's content-motion coordinates");
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
        remote::input::dispatch(ui.window(), remote::input::Input::Key { down: true, keysym: 0xffeb });
        remote::input::dispatch(ui.window(), remote::input::Input::Key { down: true, keysym: 32 });
        assert!(keys.borrow().iter().any(|(name, meta)| name == "space" && *meta),
            "closing the focused browser restores desktop Super shortcuts");
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
            assert_eq!(app.moved((i % 1280) as f32, (i % 720) as f32), Motion::Cursor);
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
}
