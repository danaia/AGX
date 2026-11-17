//! Alacritty's PTY/parser thread and bounded per-session terminal state.
use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::Dimensions,
    sync::FairMutex,
    term::{Config, Osc52},
    tty,
};
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Size {
    pub cols: usize,
    pub lines: usize,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}
impl Size {
    pub fn window(self) -> WindowSize {
        WindowSize {
            num_cols: self.cols as u16,
            num_lines: self.lines as u16,
            cell_width: 9,
            cell_height: 19,
        }
    }
}
pub(super) struct Signals {
    pub dirty: AtomicBool,
    pub exited: AtomicBool,
    pub title: Mutex<String>,
    pub size: Mutex<WindowSize>,
    sender: Mutex<Option<EventLoopSender>>,
}
#[derive(Clone)]
pub(super) struct Proxy(pub Arc<Signals>);
impl Proxy {
    pub(super) fn new(size: Size) -> Self {
        Self(Arc::new(Signals {
            dirty: AtomicBool::new(true),
            exited: AtomicBool::new(false),
            title: Mutex::new("Shell".into()),
            size: Mutex::new(size.window()),
            sender: Mutex::new(None),
        }))
    }
}
impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        let reply = match event {
            Event::Title(title) => {
                *self.0.title.lock().unwrap() = title
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(128)
                    .collect();
                None
            }
            Event::ResetTitle => {
                *self.0.title.lock().unwrap() = "Shell".into();
                None
            }
            Event::PtyWrite(text) => Some(text),
            Event::ColorRequest(index, format) => Some(format(super::render::palette(index))),
            Event::TextAreaSizeRequest(format) => Some(format(*self.0.size.lock().unwrap())),
            Event::Exit | Event::ChildExit(_) => {
                self.0.exited.store(true, Ordering::Release);
                None
            }
            _ => None,
        };
        if let Some(text) = reply.filter(|text| text.len() <= 4096) {
            if let Some(sender) = &*self.0.sender.lock().unwrap() {
                let _ = sender.send(Msg::Input(text.into_bytes().into()));
            }
        }
        self.0.dirty.store(true, Ordering::Release);
    }
}
pub(super) struct Session {
    pub term: Arc<FairMutex<Term<Proxy>>>,
    pub signals: Arc<Signals>,
    pub size: Size,
    sender: EventLoopSender,
    worker: Option<JoinHandle<()>>,
    child_handle: OwnedFd,
}
impl Session {
    pub fn new(size: Size) -> io::Result<Self> {
        if unsafe { libc::geteuid() } == 0 {
            return Err(io::Error::other(
                "Open Terminal from a signed-in ANGI session.",
            ));
        }
        let proxy = Proxy::new(size);
        let signals = proxy.0.clone();
        let config = Config {
            scrolling_history: 10_000,
            osc52: Osc52::Disabled,
            kitty_keyboard: false,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, proxy.clone())));
        let shell = std::env::var("AGX_TERMINAL_SHELL")
            .ok()
            .or_else(|| {
                std::path::Path::new("/bin/bash")
                    .is_file()
                    .then(|| "/bin/bash".into())
            })
            .or_else(|| std::env::var("SHELL").ok())
            .filter(|s| std::path::Path::new(s).is_absolute() && std::path::Path::new(s).is_file())
            .unwrap_or_else(|| "/bin/sh".into());
        let rcfile = "/usr/share/agx/terminal/bashrc";
        let args = if std::path::Path::new(&shell)
            .file_name()
            .is_some_and(|name| name == "bash")
        {
            if std::path::Path::new(rcfile).is_file() {
                vec!["--rcfile".into(), rcfile.into(), "-i".into()]
            } else {
                vec!["-i".into()]
            }
        } else {
            vec!["-l".into()]
        };
        let options = tty::Options {
            shell: Some(tty::Shell::new(shell.clone(), args)),
            working_directory: std::env::var_os("HOME").map(Into::into),
            env: [
                ("SHELL".into(), shell),
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
                ("TERM_PROGRAM".into(), "ANGI".into()),
                ("LC_CTYPE".into(), "C.UTF-8".into()),
            ]
            .into(),
            ..tty::Options::default()
        };
        let pty = tty::new(&options, size.window(), 0)?;
        let pid = pty.child().id() as i32;
        let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        let child_handle = unsafe { OwnedFd::from_raw_fd(descriptor) };
        let event_loop = EventLoop::new(term.clone(), proxy, pty, true, false)?;
        let sender = event_loop.channel();
        *signals.sender.lock().unwrap() = Some(sender.clone());
        // Drop the returned event loop/PTY immediately on child exit, even if
        // its tab remains open to inspect scrollback. Never retain a stale PID.
        let io_worker = event_loop.spawn();
        let worker = std::thread::spawn(move || {
            let _ = io_worker.join();
        });
        Ok(Self {
            term,
            signals,
            size,
            sender,
            worker: Some(worker),
            child_handle,
        })
    }
    pub fn write(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() && bytes.len() <= 1024 * 1024 {
            let _ = self.sender.send(Msg::Input(bytes.into()));
        }
    }
    pub fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        self.term.lock().resize(size);
        self.size = size;
        *self.signals.size.lock().unwrap() = size.window();
        let _ = self.sender.send(Msg::Resize(size.window()));
        self.signals.dirty.store(true, Ordering::Release);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
        // Child cleanup must not block window movement or rendering.
        if let Some(worker) = self.worker.take() {
            let handle = self.child_handle.try_clone().ok();
            std::thread::spawn(move || {
                if let Some(fd) = &handle {
                    unsafe {
                        libc::syscall(
                            libc::SYS_pidfd_send_signal,
                            fd.as_raw_fd(),
                            libc::SIGHUP,
                            std::ptr::null::<libc::siginfo_t>(),
                            0,
                        );
                    }
                }
                for _ in 0..20 {
                    if worker.is_finished() {
                        let _ = worker.join();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                // A pidfd keeps cleanup tied to this child even after its numeric PID is reused.
                if let Some(fd) = &handle {
                    unsafe {
                        libc::syscall(
                            libc::SYS_pidfd_send_signal,
                            fd.as_raw_fd(),
                            libc::SIGKILL,
                            std::ptr::null::<libc::siginfo_t>(),
                            0,
                        );
                    }
                }
                let _ = worker.join();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_a_tab_terminates_a_shell_that_ignores_hangup() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let session = Session::new(Size {
            cols: 80,
            lines: 24,
        })
        .unwrap();
        let handle = session.child_handle.try_clone().unwrap();
        session.write(b"trap '' HUP; read -r ANGI_TEST_BLOCK\r".to_vec());
        std::thread::sleep(std::time::Duration::from_millis(80));
        drop(session);
        let mut poll = libc::pollfd {
            fd: handle.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        assert_eq!(
            unsafe { libc::poll(&mut poll, 1, 3000) },
            1,
            "closing the tab must not leave its shell alive"
        );
        assert_ne!(poll.revents & libc::POLLIN, 0);
    }
}
