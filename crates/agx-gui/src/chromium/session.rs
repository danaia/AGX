//! Keep Chromium, Cage and WayVNC out of the desktop event loop.
use super::rfb::{Client, Frame, Input};
use std::{
    io::{self, BufRead, BufReader},
    os::{fd::AsRawFd, unix::net::UnixStream},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

pub(super) struct Session {
    pub(super) input: SyncSender<Input>,
    frame: Arc<Mutex<Option<Frame>>>,
    pub(super) result: Receiver<io::Result<()>>,
    stop: Arc<AtomicBool>,
}

impl Session {
    pub(super) fn launch(arguments: Vec<String>) -> Self {
        let (input, commands) = mpsc::sync_channel(256);
        let frame = Arc::new(Mutex::new(None));
        let output = frame.clone();
        let (done, result) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = stop.clone();
        std::thread::spawn(move || {
            let outcome = run(arguments, commands, &output, &stop_worker);
            let _ = done.send(outcome);
        });
        Self {
            input,
            frame,
            result,
            stop,
        }
    }

    pub(super) fn take_frame(&self) -> Option<Frame> {
        self.frame.try_lock().ok()?.take()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // The private host handles SIGTERM and cleans up its children and runtime directory.
            unsafe { libc::kill(self.0.id() as i32, libc::SIGTERM) };
            let _ = self.0.wait();
        }
    }
}

fn run(
    arguments: Vec<String>,
    commands: Receiver<Input>,
    output: &Mutex<Option<Frame>>,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut host = Host(
        Command::new("/usr/bin/python3")
            .args(["/usr/libexec/agx-wayland/session.py", "--serve"])
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?,
    );
    let stdout = host.0.stdout.take().expect("host stdout is piped");
    let mut pending = BufReader::new(stdout);
    while !readable(pending.get_ref(), 30)? {
        if stop.load(Ordering::Relaxed) || host.0.try_wait()?.is_some() {
            return Ok(());
        }
    }
    let mut path = String::new();
    if pending.read_line(&mut path)? == 0 {
        return Err(io::Error::other("the Wayland session did not start"));
    }
    let socket = UnixStream::connect(path.trim_end())?;
    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
    socket.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut client = Client::connect(socket)?;
    client.request(false)?;
    while !stop.load(Ordering::Relaxed) {
        for input in commands.try_iter().take(64) {
            input.write(&mut client.socket)?;
        }
        if !readable(&client.socket, 16)? {
            continue;
        }
        if let Some(frame) = client.receive()? {
            *output.lock().expect("only the framebuffer slot is locked") = Some(frame);
            client.request(true)?;
        }
    }
    Ok(())
}

fn readable(source: &impl AsRawFd, timeout: i32) -> io::Result<bool> {
    let mut poll = libc::pollfd {
        fd: source.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // POLLHUP also wakes the reader, which then reports EOF.
    match unsafe { libc::poll(&mut poll, 1, timeout) } {
        -1 if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => Ok(false),
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(false),
        _ => Ok(true),
    }
}
