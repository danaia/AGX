//! AGX desktop: a Slint front end over the same system data as the TUI, with
//! Omarchy/Hyprland-style window management (see wm.rs and keymap.rs).
//!
//! Exit codes let a launcher script react to the user's choice:
//! 0 = quit, 10 = the user asked for the TUI instead.
mod keymap;
mod wm;
mod scene_model;
mod login;
mod graphics;

use agx_core::{Sampler, Snapshot};
use keymap::{Cmd, ModKey, Mods, bind};
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
use wm::{Button, Desktop, Edges, Full, Hit, Kind, Mode, Rect};

slint::include_modules!();

const LOGO: &str = include_str!("../../../brand/logo-ascii.md");
const EXIT_TUI: i32 = 10;
const TOP: f32 = 34.0;
const BOTTOM: f32 = 26.0;
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

enum Drag {
    None,
    /// Tiled windows are dragged as a ghost and swapped on drop; floating ones move.
    Move { kind: Kind, tiled: bool, start: (f32, f32) },
    ResizeTiled { kind: Kind, sx: f32, sy: f32 },
    ResizeFloat { kind: Kind, edges: Edges },
    Split { split: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion { Cursor, Overlay, Geometry }

struct App {
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
}

fn owner(hit: Hit) -> Option<Kind> {
    match hit {
        Hit::Button(k, _) | Hit::Header(k) | Hit::Edge(k, _) | Hit::Body(k) => Some(k),
        Hit::Handle(_) | Hit::None => None,
    }
}

impl App {
    fn new() -> Self {
        let screen = Rect::new(0.0, 0.0, 1280.0, 720.0);
        let desk = Desktop::new(screen, Rect::new(0.0, TOP, 1280.0, 720.0 - TOP - BOTTOM));
        let view = SceneModel::default();
        view.sync(desk.scene(), None);
        Self {
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
        for i in 0..wm::WORKSPACES {
            let occupied = self.desk.occupied(i);
            if self.occupied.row_data(i) != Some(occupied) {
                self.occupied.set_row_data(i, occupied);
            }
        }
        ui.set_workspace(self.desk.current() as i32);
        ui.set_layout_name(
            match (self.desk.fullscreen_of(), self.desk.mode()) {
                (Some((_, Full::Screen)), _) => "fullscreen",
                (Some((_, Full::Maximized)), _) => "maximized",
                (None, Mode::Tiled) => "tiled",
                (None, Mode::Floating) => "window mode",
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
        let m = self.modk.name();
        match cmd {
            Cmd::Focus(d) => self.desk.focus_dir(d),
            Cmd::Swap(d) => self.desk.swap_dir(d),
            Cmd::Close => {
                if let Some(k) = self.desk.focused() {
                    self.desk.close(k);
                    self.toast(ui, &format!("Closed {}. Reopen it from the menu ({m}+Space).", k.title()));
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

    fn menu_items(&self) -> Vec<(String, String, Cmd)> {
        let m = self.modk.name();
        let mut items = vec![];
        for n in 0..wm::WORKSPACES {
            items.push((format!("Workspace {}", n + 1), format!("{m}+{}", n + 1), Cmd::Workspace(n)));
        }
        for k in Kind::ALL {
            if !self.desk.is_open(k) {
                items.push((format!("Open {}", k.title()), "closed panel".into(), Cmd::Open(k)));
            }
        }
        if let Some(f) = self.desk.focused() {
            let t = f.title();
            items.push((format!("Close {t}"), format!("{m}+Q"), Cmd::Close));
            items.push((format!("Float or tile {t}"), format!("{m}+T"), Cmd::ToggleFloat));
            items.push((format!("Fullscreen {t}"), format!("{m}+F"), Cmd::Fullscreen));
            items.push((format!("Maximize {t}"), "full width".into(), Cmd::Maximize));
            items.push(("Toggle split".into(), format!("{m}+J"), Cmd::ToggleSplit));
        }
        items.push((
            if self.desk.mode() == Mode::Tiled { "Window mode (float all)" } else { "Tile this workspace" }.into(),
            format!("{m}+L"),
            Cmd::CycleLayout,
        ));
        items.push(("Open TUI".into(), "terminal console".into(), Cmd::Tui));
        items.push(("Sign out".into(), "return to login".into(), Cmd::Logout));
        items.push(("Reboot".into(), "asks first".into(), Cmd::Reboot));
        items.push(("Shut down".into(), "asks first".into(), Cmd::Shutdown));
        items
    }

    fn set_menu_rows(&mut self, ui: &AppWindow, query: &str) {
        let q = query.to_lowercase();
        let (mut rows, mut cmds) = (vec![], vec![]);
        for (label, hint, cmd) in self.menu_items() {
            if label.to_lowercase().contains(&q) {
                rows.push(MenuRow { label: label.into(), hint: hint.into() });
                cmds.push(cmd);
            }
        }
        ui.set_menu_rows(ModelRc::new(VecModel::from(rows)));
        self.menu = cmds;
    }

    fn open_menu(&mut self, ui: &AppWindow) {
        self.set_menu_rows(ui, "");
        ui.set_menu_index(0);
        ui.set_menu_open(true);
    }

    fn menu_accept(&mut self, ui: &AppWindow, index: i32) {
        if let Some(cmd) = self.menu.get(index as usize).copied() {
            self.run(ui, cmd);
        }
    }

    fn confirm(&mut self, ui: &AppWindow, yes: bool) {
        ui.set_confirm_text("".into());
        let cmd = self.pending.take();
        if yes {
            power(ui, cmd);
        }
    }

    fn key(&mut self, ui: &AppWindow, name: &str, mods: Mods) -> bool {
        // A key press always ends a drag, so a lost button release cannot strand one.
        if !matches!(self.drag, Drag::None) {
            self.drag = Drag::None;
            self.ghost = None;
            self.target = None;
            self.refresh(ui);
        }
        match bind(name, mods, self.modk) {
            Some(cmd) => {
                self.run(ui, cmd);
                true
            }
            None => false,
        }
    }

    // --- pointer ------------------------------------------------------------
    fn pointer(&mut self, ui: &AppWindow, kind: i32, button: i32, x: f32, y: f32, mods: Mods) {
        match kind {
            0 => {
                self.down(ui, button, x, y, mods);
                self.refresh(ui);
            }
            1 | 3 => {
                // A release can carry a newer position than the last move event.
                if kind == 1 { self.moved(x, y); }
                self.up(x, y, kind == 1);
                self.refresh(ui);
            }
            _ => match self.moved(x, y) {
                Motion::Cursor => ui.set_cursor(self.cursor),
                Motion::Overlay => {
                    self.view.target(self.target);
                    self.refresh_drag(ui);
                }
                Motion::Geometry => {
                    self.view.sync(self.desk.scene(), self.target);
                    self.refresh_drag(ui);
                }
            },
        }
    }

    fn begin_move(&mut self, k: Kind, x: f32, y: f32) {
        if self.desk.fullscreen_of().is_some() {
            return;
        }
        self.drag = Drag::Move { kind: k, tiled: !self.desk.is_floating(k), start: (x, y) };
    }

    fn begin_resize(&mut self, k: Kind, x: f32, y: f32) {
        if self.desk.fullscreen_of().is_some() {
            return;
        }
        self.drag = if self.desk.is_floating(k) {
            Drag::ResizeFloat { kind: k, edges: Edges { right: true, bottom: true, ..Default::default() } }
        } else {
            let (cx, cy) = self.view.scene.borrow().rect_of(k).map_or((x, y), |r| r.center());
            Drag::ResizeTiled { kind: k, sx: if x < cx { -1.0 } else { 1.0 }, sy: if y < cy { -1.0 } else { 1.0 } }
        };
    }

    fn down(&mut self, ui: &AppWindow, button: i32, x: f32, y: f32, mods: Mods) {
        self.mods = mods;
        self.last = (x, y);
        // The top bar's workspace buttons: 30px wide at a 32px pitch, from x = 10.
        if button == 0 && y < TOP && !matches!(self.desk.fullscreen_of(), Some((_, Full::Screen))) {
            let i = ((x - 10.0) / 32.0).floor();
            if i >= 0.0 && (x - 10.0 - i * 32.0) < 30.0 && (i as usize) < wm::WORKSPACES {
                self.run(ui, Cmd::Workspace(i as usize));
            }
            return;
        }
        let hit = self.view.scene.borrow().hit(x, y);
        let held = self.modk.held(mods);
        match button {
            0 => {
                if held && let Some(k) = owner(hit) {
                    self.desk.focus(k);
                    self.begin_move(k, x, y);
                    return;
                }
                match hit {
                    Hit::Button(k, b) => {
                        self.desk.focus(k);
                        self.run(
                            ui,
                            match b {
                                Button::Float => Cmd::ToggleFloat,
                                Button::Full => Cmd::Fullscreen,
                                Button::Close => Cmd::Close,
                            },
                        );
                    }
                    Hit::Header(k) => {
                        self.desk.focus(k);
                        self.begin_move(k, x, y);
                    }
                    Hit::Edge(k, e) => {
                        self.desk.focus(k);
                        self.drag = Drag::ResizeFloat { kind: k, edges: e };
                    }
                    Hit::Handle(h) => self.drag = Drag::Split { split: h.split },
                    Hit::Body(k) => self.desk.focus(k),
                    Hit::None => {}
                }
            }
            1 => {
                if held && let Some(k) = owner(hit) {
                    self.desk.focus(k);
                    self.begin_resize(k, x, y);
                } else if let Hit::Header(k) = hit {
                    self.desk.focus(k);
                    self.open_menu(ui);
                }
            }
            _ => {}
        }
    }

    fn moved(&mut self, x: f32, y: f32) -> Motion {
        let (dx, dy) = (x - self.last.0, y - self.last.1);
        self.last = (x, y);
        match self.drag {
            Drag::None => self.cursor = self.hover(x, y),
            Drag::Move { kind, tiled: false, .. } => self.desk.move_floating(kind, dx, dy),
            Drag::Move { kind, tiled: true, start } => {
                self.ghost = self.view.scene.borrow().rect_of(kind).map(|r| Rect::new(r.x + x - start.0, r.y + y - start.1, r.w, r.h));
                self.target = self.view.scene.borrow().tile_at(x, y).filter(|t| *t != kind);
            }
            Drag::ResizeTiled { kind, sx, sy } => self.desk.resize_window(kind, sx * dx, sy * dy),
            Drag::ResizeFloat { kind, edges } => self.desk.resize_floating_edges(kind, edges, dx, dy),
            Drag::Split { split } => {
                if let Some(r) = self.view.scene.borrow().handle_ratio(split, x, y) {
                    self.desk.set_split(split, r);
                }
            }
        }
        match self.drag {
            Drag::None => Motion::Cursor,
            Drag::Move { tiled: true, .. } => Motion::Overlay,
            _ => Motion::Geometry,
        }
    }

    fn up(&mut self, x: f32, y: f32, commit: bool) {
        if commit
            && let Drag::Move { kind, tiled: true, .. } = self.drag
            && let Some(t) = self.view.scene.borrow().tile_at(x, y).filter(|t| *t != kind)
        {
            self.desk.swap(kind, t);
        }
        self.drag = Drag::None;
        self.ghost = None;
        self.target = None;
        self.cursor = self.hover(x, y);
    }

    fn hover(&self, x: f32, y: f32) -> i32 {
        if y < TOP {
            return 6;
        }
        match self.view.scene.borrow().hit(x, y) {
            Hit::Header(_) => 1,
            Hit::Button(..) => 6,
            Hit::Handle(h) => if h.axis == wm::Axis::Horizontal { 2 } else { 3 },
            Hit::Edge(_, e) => match (e.left || e.right, e.top || e.bottom) {
                (true, true) => if (e.left && e.top) || (e.right && e.bottom) { 4 } else { 5 },
                (true, false) => 2,
                _ => 3,
            },
            _ => 0,
        }
    }

    fn scroll(&mut self, ui: &AppWindow, delta: f32, mods: Mods) {
        if self.modk.held(mods) && delta != 0.0 {
            self.desk.next_workspace(delta > 0.0);
            self.refresh(ui);
        }
    }
}

fn mods(ctrl: bool, alt: bool, shift: bool, meta: bool) -> Mods {
    Mods { ctrl, alt, shift, meta }
}

fn main() -> ExitCode {
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
    hook!(on_scroll, |a, ui, delta, ctrl, alt, shift, meta| a.scroll(&ui, delta, mods(ctrl, alt, shift, meta)));
    hook!(on_resized, |a, ui, w, h| a.resized(&ui, w, h));
    hook!(on_menu_query, |a, ui, query| a.set_menu_rows(&ui, &query));
    hook!(on_menu_accept, |a, ui, index| a.menu_accept(&ui, index));
    hook!(on_confirm, |a, ui, yes| a.confirm(&ui, yes));
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
