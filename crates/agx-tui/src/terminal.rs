//! Terminal mode, signal registrations and hangup recovery.
use ratatui::crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io;

pub(super) struct Session {
    active: bool,
}
impl Session {
    pub(super) fn enter(mouse: bool) -> io::Result<Self> {
        let session = Self { active: true };
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        if mouse {
            execute!(io::stdout(), EnableMouseCapture)?;
        }
        Ok(session)
    }
    pub(super) fn suspend(&mut self) {
        restore();
        self.active = false;
    }
    pub(super) fn resume(&mut self, mouse: bool) -> io::Result<()> {
        // A child app can enable input reporting even if the TUI disables it.
        execute!(
            io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            Show
        )?;
        *self = Self::enter(mouse)?;
        Ok(())
    }
}
pub(super) fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    let _ = disable_raw_mode();
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.active {
            restore();
        }
    }
}
pub(super) struct Signals(pub(super) Vec<signal_hook::SigId>);
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

pub(super) fn terminal_closed() -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // descriptor is initialized, count is one, and a zero timeout never blocks.
    if unsafe { libc::poll(&mut descriptor, 1, 0) } < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error);
    }
    Ok(descriptor.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0)
}

pub(super) fn gui_available() -> bool {
    std::fs::read_link("/proc/self/fd/0")
        .is_ok_and(|path| path == std::path::Path::new("/dev/tty1"))
        && std::path::Path::new("/usr/bin/agx-gui").is_file()
}
