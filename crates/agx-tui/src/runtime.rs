//! Input dispatch and session lifecycle for the interactive console.
use crate::terminal::{Session, Signals, restore, terminal_closed};
use crate::worker::Worker;
use crate::{Exit, Options, app::App, profile, services, wifi};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::event::{
        self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
};
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wifi::Interaction;

pub fn run(options: Options) -> io::Result<Exit> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "The TUI requires an interactive terminal. Use ssh -t HOST agx tui, or agx status --json for scripts.",
        ));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let mut signals = Signals(Vec::new());
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ] {
        signals
            .0
            .push(signal_hook::flag::register(signal, Arc::clone(&stop))?);
    }
    static PANIC_HOOK: std::sync::Once = std::sync::Once::new();
    PANIC_HOOK.call_once(|| {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous_hook(info);
        }));
    });
    let worker = Worker::start()?;
    let wifi_worker = wifi::Worker::start()?;
    let profile_worker = profile::Worker::start()?;
    let service_worker = services::Worker::start()?;
    let mut session = Session::enter(options.mouse)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut app = App::new(options);
    let mut dirty = true;
    let mut last_frame = Instant::now();
    let mut power_operation = None;
    let mut exit = Exit::Quit;
    while !stop.load(Ordering::Relaxed) && !terminal_closed()? {
        while let Ok(result) = worker.results.try_recv() {
            match result {
                Ok(snapshot) => {
                    app.snapshot = Some(snapshot);
                    app.error = None;
                    app.last_success = Some(Instant::now());
                }
                Err(error) => app.error = Some(error),
            }
            dirty = true;
        }
        while let Ok(update) = wifi_worker.results.try_recv() {
            if let Some(reboot) = update.power_accepted {
                power_operation = Some(reboot);
            }
            app.wifi.update(update);
            dirty = true;
        }
        if power_operation.is_some() {
            break;
        }
        while let Ok(update) = profile_worker.results.try_recv() {
            app.profile.update(update);
            dirty = true;
        }
        while let Ok(update) = service_worker.results.try_recv() {
            app.services.update(update);
            dirty = true;
        }
        if app.system.poll() {
            dirty = true;
        }
        if dirty || last_frame.elapsed() >= Duration::from_secs(1) {
            terminal.draw(|frame| app.draw(frame))?;
            dirty = false;
            last_frame = Instant::now();
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        if terminal_closed()? {
            break;
        }
        dirty = true;
        let input = event::read()?;
        if matches!(&input, Event::Key(key) if key.kind == KeyEventKind::Release) {
            continue;
        }
        if matches!(&input, Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
        {
            break;
        }
        if app.wifi.has_popup() {
            if let Interaction::Send(command) = app.wifi.popup_event(&input) {
                wifi_worker.send(command, &mut app.wifi);
            }
            continue;
        }
        let wants_gui = matches!(&input, Event::Key(key) if key.code == KeyCode::F(8))
            || matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && (app.gui_footer.contains((mouse.column, mouse.row).into())
                    || (app.page == 0 && app.profile.gui_button.contains((mouse.column, mouse.row).into()))));
        if wants_gui {
            if !app.gui_available {
                app.wifi.notice = Some("GUI is available on the HDMI console (tty1).".into());
            } else if app.profile.editing() {
                app.wifi.notice =
                    Some("Save or discard Profile edits before returning to GUI.".into());
            } else if (app.page == 5 || app.page == 6) && app.services.popup()
                || app.page == 4 && app.system.dialog_open()
            {
                // Let the existing dialog finish before leaving its event loop.
            } else {
                exit = Exit::Gui;
                break;
            }
            continue;
        }
        let shell_request = if matches!(&input,Event::Key(key) if key.code==KeyCode::F(4))
            || matches!(&input,Event::Mouse(mouse) if mouse.kind==MouseEventKind::Down(MouseButton::Left) && app.shell_footer.contains((mouse.column,mouse.row).into()))
        {
            Some(crate::shell::Kind::Shell)
        } else if app.page == 7 {
            match &input {
                Event::Key(key) if key.code == KeyCode::Enter => Some(crate::shell::Kind::Shell),
                Event::Key(key) if key.code == KeyCode::Char('t') => Some(crate::shell::Kind::Tmux),
                Event::Key(key) if key.code == KeyCode::Char('o') => {
                    Some(crate::shell::Kind::OpenCode)
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && app.shell_button.contains((mouse.column, mouse.row).into()) =>
                {
                    Some(crate::shell::Kind::Shell)
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && app.tmux_button.contains((mouse.column, mouse.row).into()) =>
                {
                    Some(crate::shell::Kind::Tmux)
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && app
                            .opencode_button
                            .contains((mouse.column, mouse.row).into()) =>
                {
                    Some(crate::shell::Kind::OpenCode)
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(kind) = shell_request {
            if app.profile.editing() || app.system.dialog_open() || app.services.popup() {
                app.wifi.notice =
                    Some("Finish the current edit or dialog before opening Terminal.".into());
                continue;
            }
            session.suspend();
            let result = crate::shell::run(kind, &stop);
            if stop.load(Ordering::Relaxed) || terminal_closed()? {
                break;
            }
            session.resume(options.mouse)?;
            terminal.autoresize()?;
            terminal.clear()?;
            app.wifi.notice = Some(match result {
                Ok(status) if status.success() => "Returned from Terminal.".into(),
                Ok(status) => format!("Terminal finished: {status}"),
                Err(error) => format!("Unable to open Terminal: {error}"),
            });
            let _ = worker.refresh.try_send(());
            continue;
        }
        if app.page == 0 {
            let (consumed, command) = app.profile.event(&input);
            if let Some(command) = command {
                profile_worker.send(command, &mut app.profile);
            }
            if consumed {
                continue;
            }
        }
        if (app.page == 5 || app.page == 6) && app.services.popup() {
            if let Some(command) = app.services.event(&input, app.page == 6) {
                service_worker.send(command, &mut app.services);
            }
            continue;
        }
        if (app.page == 5 || app.page == 6)
            && let Some(command) = app.services.event(&input, app.page == 6)
        {
            service_worker.send(command, &mut app.services);
        }
        if app.page == 4 && app.system.dialog_open() {
            app.system.dialog_input(&input);
            continue;
        }
        if app.page == 4
            && (matches!(&input, Event::Key(key) if key.code == KeyCode::Char('g'))
                || matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.system.token_button.contains((mouse.column,mouse.row).into())))
        {
            app.system.token_dialog();
            continue;
        }
        if app.page == 4
            && (matches!(&input, Event::Key(key) if key.code == KeyCode::Char('v'))
                || matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.system.staged_button.contains((mouse.column,mouse.row).into())))
        {
            app.system.start_staged();
            continue;
        }
        if app.page == 4
            && (matches!(&input, Event::Key(key) if key.code == KeyCode::Char('c'))
                || matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.system.button.contains((mouse.column, mouse.row).into())))
        {
            app.system.start_github();
            continue;
        }
        if matches!(&input, Event::Key(key) if key.code == KeyCode::F(9))
            || matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.wifi.shutdown_area.contains((mouse.column,mouse.row).into()))
            || (app.page == 4
                && matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.system.shutdown_button.contains((mouse.column,mouse.row).into())))
        {
            app.wifi.shutdown();
            continue;
        }
        if matches!(&input, Event::Key(key) if key.code == KeyCode::F(10)) {
            app.wifi.reboot();
            continue;
        }
        if matches!(&input, Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) && app.wifi.reboot_area.contains((mouse.column, mouse.row).into()))
        {
            app.wifi.reboot();
            continue;
        }
        if app.page == 3
            && let Event::Key(key) = &input
        {
            match app.wifi.key(*key) {
                Interaction::Consumed => continue,
                Interaction::Send(command) => {
                    wifi_worker.send(command, &mut app.wifi);
                    continue;
                }
                Interaction::Unused => (),
            }
        }
        match input {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('1') => app.page(0),
                    KeyCode::Char('2') => app.page(1),
                    KeyCode::Char('3') => app.page(2),
                    KeyCode::Char('5') => app.page(4),
                    KeyCode::Char('6') => app.page(5),
                    KeyCode::Char('7') => app.page(6),
                    KeyCode::Char('?') => app.page(8),
                    KeyCode::Char('8') => app.page(7),
                    KeyCode::Char('4') => {
                        app.page(3);
                        if let Some(command) = app.wifi.entered() {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    }
                    KeyCode::Tab | KeyCode::Right => {
                        app.page(app.page + 1);
                        if app.page == 3
                            && let Some(command) = app.wifi.entered()
                        {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    }
                    KeyCode::BackTab | KeyCode::Left => {
                        app.page(app.page + 8);
                        if app.page == 3
                            && let Some(command) = app.wifi.entered()
                        {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => app.move_row(1),
                    KeyCode::Up | KeyCode::Char('k') => app.move_row(-1),
                    KeyCode::PageDown => app.move_row(8),
                    KeyCode::PageUp => app.move_row(-8),
                    KeyCode::Home => {
                        app.scroll = 0;
                        if app.page == 3 {
                            app.wifi.home_end(false);
                        } else {
                            app.table.select(Some(0));
                        }
                    }
                    KeyCode::End => {
                        let count = app.snapshot.as_ref().map_or(0, |s| s.hardware.len());
                        if app.page == 3 {
                            app.wifi.home_end(true);
                        } else {
                            app.table.select(count.checked_sub(1));
                        }
                    }
                    KeyCode::Char('r') => {
                        let _ = worker.refresh.try_send(());
                    }
                    _ => (),
                }
            }
            Event::Mouse(mouse) if options.mouse => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(page) = app
                        .tabs
                        .iter()
                        .position(|rect| rect.contains((mouse.column, mouse.row).into()))
                    {
                        app.page(page);
                        if page == 3
                            && let Some(command) = app.wifi.entered()
                        {
                            wifi_worker.send(command, &mut app.wifi);
                        }
                    } else if app.page == 2
                        && app.table_area.contains((mouse.column, mouse.row).into())
                        && mouse.row >= app.table_area.y + 2
                        && mouse.row < app.table_area.bottom().saturating_sub(1)
                    {
                        let index =
                            app.table.offset() + (mouse.row - app.table_area.y - 2) as usize;
                        if app
                            .snapshot
                            .as_ref()
                            .is_some_and(|s| index < s.hardware.len())
                        {
                            app.table.select(Some(index));
                        }
                    } else if app.page == 3
                        && let Interaction::Send(command) = app.wifi.mouse(mouse)
                    {
                        wifi_worker.send(command, &mut app.wifi);
                    }
                }
                MouseEventKind::ScrollDown => app.move_row(3),
                MouseEventKind::ScrollUp => app.move_row(-3),
                _ => (),
            },
            Event::Resize(_, _) => (),
            _ => (),
        }
    }
    drop(terminal);
    drop(session);
    if let Some(rebooting) = power_operation {
        use std::io::Write;
        let mut output = io::stdout().lock();
        let _ = writeln!(
            output,
            "\nANGI is shutting down{}. Your SSH session will close.",
            if rebooting { " for reboot" } else { "" }
        );
        if let Ok(logo) = std::fs::read_to_string("/usr/share/agx/logo.ascii") {
            for line in logo.lines() {
                let _ = writeln!(output, "{line}");
                let _ = output.flush();
                std::thread::sleep(Duration::from_millis(35));
            }
        }
        let _ = writeln!(
            output,
            "\n{}",
            if rebooting {
                "Please log back in once ANGI is ready."
            } else {
                "Wait for the Pi to finish shutting down before unplugging power."
            }
        );
        let _ = output.flush();
    }
    Ok(exit)
}
