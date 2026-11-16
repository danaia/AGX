//! Startup chooser: asks whether to enter the desktop (GUI) or the console (TUI).
//!
//! The prompt is plain terminal I/O so it works on the HDMI console (tty1)
//! before any graphics stack runs. Timeout and Enter pick the console, which
//! keeps unattended and headless boots behaving as they did before the GUI.
use std::{
    io::{self, Write},
    os::fd::AsRawFd,
    path::Path,
    time::{Duration, Instant},
};

/// `agx-gui` exits with this code when the user picks "Open TUI" inside it.
pub const GUI_WANTS_TUI: i32 = 10;
const GUI_PATHS: [&str; 2] = ["/usr/bin/agx-gui", "/usr/local/bin/agx-gui"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Gui,
    Tui,
}

/// Switch interfaces in one authenticated CLI process, without nested launchers.
pub fn session_loop(
    mut choice: Choice,
    mut run: impl FnMut(Choice) -> io::Result<Option<Choice>>,
) -> io::Result<()> {
    loop {
        match run(choice)? {
            Some(next) => choice = next,
            None => return Ok(()),
        }
    }
}

pub fn gui_installed() -> bool {
    GUI_PATHS.iter().any(|p| Path::new(p).is_file())
}

/// Map a key to a choice; `None` means keep waiting.
pub fn parse_key(byte: u8, default: Choice) -> Option<Choice> {
    match byte {
        b'1' | b'g' | b'G' => Some(Choice::Gui),
        b'2' | b't' | b'T' => Some(Choice::Tui),
        b'\r' | b'\n' => Some(default),
        _ => None,
    }
}

struct RawTerminal {
    fd: i32,
    saved: libc::termios,
}
impl RawTerminal {
    fn enter(fd: i32) -> io::Result<Self> {
        // SAFETY: tcgetattr/tcsetattr only read and write the termios struct we own.
        unsafe {
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut saved) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut raw = saved;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO);
            raw.c_cc[libc::VMIN] = 0;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { fd, saved })
        }
    }
}
impl Drop for RawTerminal {
    fn drop(&mut self) {
        // SAFETY: restores the attributes captured in enter().
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved) };
    }
}

fn read_byte(fd: i32, wait: Duration) -> Option<u8> {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd; read fills one byte into a local.
    unsafe {
        if libc::poll(&mut poll, 1, wait.as_millis() as i32) <= 0 {
            return None;
        }
        let mut byte = 0u8;
        (libc::read(fd, (&mut byte as *mut u8).cast(), 1) == 1).then_some(byte)
    }
}

/// Ask the user. Returns the default when the timeout expires.
pub fn ask(default: Choice, timeout: Duration) -> io::Result<Choice> {
    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    let _raw = RawTerminal::enter(fd)?;
    let mut out = io::stdout();
    let name = |c| {
        if c == Choice::Gui {
            "Desktop"
        } else {
            "Console"
        }
    };
    write!(
        out,
        "\x1b[2J\x1b[H\n  \x1b[1;36mAGX OS\x1b[0m  choose your interface\n\n\
         \x20   \x1b[1;32m1\x1b[0m  Desktop  (GUI)\n\
         \x20   \x1b[1;32m2\x1b[0m  Console  (TUI)\n\n"
    )?;
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        write!(
            out,
            "\r\x1b[K  Enter = {}. Starting {} in {}s ",
            name(default),
            name(default),
            left.as_secs() + 1
        )?;
        out.flush()?;
        if left.is_zero() {
            writeln!(out)?;
            return Ok(default);
        }
        if let Some(byte) = read_byte(fd, left.min(Duration::from_millis(250)))
            && let Some(choice) = parse_key(byte, default)
        {
            writeln!(out)?;
            return Ok(choice);
        }
    }
}

/// After the GUI exits, however it exited: back to text mode, cursor on, stray keys dropped.
/// A GUI killed outright leaves the console in graphics mode, which hides the shell.
pub fn reset_console() {
    const KDSETMODE: libc::c_ulong = 0x4B3A;
    const KD_TEXT: libc::c_int = 0;
    // SAFETY: plain ioctl and flush on stdin; both fail harmlessly when it is not a VT.
    unsafe {
        libc::ioctl(0, KDSETMODE, KD_TEXT);
        libc::tcflush(0, libc::TCIFLUSH);
    }
    let mut out = io::stdout();
    let _ = write!(out, "\x1b[?25h\x1b[2J\x1b[H");
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_gui_tui_switches_end_with_logout_without_nested_sessions() {
        let mut seen = Vec::new();
        session_loop(Choice::Gui, |choice| {
            seen.push(choice);
            Ok(if seen.len() == 1001 {
                None
            } else {
                Some(if choice == Choice::Gui {
                    Choice::Tui
                } else {
                    Choice::Gui
                })
            })
        })
        .unwrap();
        assert_eq!(seen.len(), 1001);
        assert_eq!(seen.last(), Some(&Choice::Gui));
        assert!(seen.windows(2).all(|pair| pair[0] != pair[1]));
    }
    #[test]
    fn keys_choose_interfaces() {
        assert_eq!(parse_key(b'1', Choice::Tui), Some(Choice::Gui));
        assert_eq!(parse_key(b'g', Choice::Tui), Some(Choice::Gui));
        assert_eq!(parse_key(b'2', Choice::Gui), Some(Choice::Tui));
        assert_eq!(parse_key(b'\r', Choice::Tui), Some(Choice::Tui));
        assert_eq!(parse_key(b'x', Choice::Tui), None);
    }
}
