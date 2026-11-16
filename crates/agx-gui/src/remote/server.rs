//! TCP side of the VNC server: accepts clients, keeps the latest captured frame, and
//! streams changed areas to each client from its own thread. Nothing here touches Slint;
//! the UI thread publishes frames and receives input through [`Shared`].
use super::{
    frame::{Frame, changed_bounds},
    handoff::{self, Handoff, Viewer},
    input::{Input, pointer_inputs},
    rfb::{self, ClientMessage, ENCODING_DESKTOP_SIZE, ENCODING_RAW, PixelFormat},
};
use std::{
    collections::HashMap,
    io::{self, BufWriter, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    os::fd::AsRawFd,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const MAX_CLIENTS: usize = 4;
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const SERVER_NAME: &str = "ANGI";

struct ClientState {
    /// An update request is outstanding (or the client is still connecting).
    pending: bool,
    /// The outstanding request asked for the whole window, not only changes.
    full: bool,
    format: PixelFormat,
    desktop_size: bool,
    closed: bool,
    /// The connection and the picture size the viewer has, once connected; kept so a
    /// new process can adopt the viewer (see `handoff`).
    stream: Option<TcpStream>,
    size: (usize, usize),
}

#[derive(Default)]
struct Hub {
    frame: Option<Arc<Frame>>,
    sequence: u64,
    clients: HashMap<u64, ClientState>,
    next_id: u64,
}

/// State shared between the UI thread and the network threads.
pub struct Shared {
    hub: Mutex<Hub>,
    changed: Condvar,
    input: Box<dyn Fn(Input) + Send + Sync>,
    started: Instant,
    last_input_ms: AtomicU64,
    listener: Mutex<Option<TcpListener>>,
}

impl Shared {
    pub fn new(input: impl Fn(Input) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            hub: Mutex::default(),
            changed: Condvar::new(),
            input: Box::new(input),
            started: Instant::now(),
            last_input_ms: AtomicU64::new(0),
            listener: Mutex::new(None),
        })
    }

    fn hub(&self) -> MutexGuard<'_, Hub> {
        self.hub
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether some client is waiting for a frame, so the UI thread should capture one.
    pub fn wants_frame(&self) -> bool {
        self.hub()
            .clients
            .values()
            .any(|client| client.pending && !client.closed)
    }

    /// Input arrived in the last couple of seconds, so frames should follow quickly.
    pub fn input_is_recent(&self) -> bool {
        let last = self.last_input_ms.load(Ordering::Relaxed);
        last != 0 && self.started.elapsed().as_millis() as u64 - last < 2000
    }

    pub fn publish(&self, frame: Frame) {
        let mut hub = self.hub();
        if hub.frame.as_deref() == Some(&frame) {
            return;
        }
        hub.frame = Some(Arc::new(frame));
        hub.sequence += 1;
        self.changed.notify_all();
    }

    fn register(&self) -> Option<u64> {
        self.register_with(PixelFormat::NATIVE, false, (0, 0))
    }

    fn register_with(
        &self,
        format: PixelFormat,
        desktop_size: bool,
        size: (usize, usize),
    ) -> Option<u64> {
        let mut hub = self.hub();
        if hub.clients.len() >= MAX_CLIENTS {
            return None;
        }
        hub.next_id += 1;
        let id = hub.next_id;
        hub.clients.insert(
            id,
            ClientState {
                pending: true,
                full: true,
                format,
                desktop_size,
                closed: false,
                stream: None,
                size,
            },
        );
        Some(id)
    }

    /// Describe the listener and connected viewers so the next process can adopt them.
    /// The caller must keep these descriptors open across its exec.
    pub fn handoff(&self) -> Option<Handoff> {
        let listener = self
            .listener
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()?
            .as_raw_fd();
        let viewers = self
            .hub()
            .clients
            .values()
            .filter(|client| !client.closed)
            .filter_map(|client| {
                Some(Viewer {
                    fd: client.stream.as_ref()?.as_raw_fd(),
                    size: client.size,
                    desktop_size: client.desktop_size,
                    format: client.format,
                })
            })
            .collect();
        Some(Handoff { listener, viewers })
    }

    fn unregister(&self, id: u64) {
        let mut hub = self.hub();
        hub.clients.remove(&id);
        if hub.clients.is_empty() {
            hub.frame = None;
        }
    }

    fn update_client(&self, id: u64, change: impl FnOnce(&mut ClientState)) {
        if let Some(client) = self.hub().clients.get_mut(&id) {
            change(client);
        }
        self.changed.notify_all();
    }

    fn first_frame(&self) -> io::Result<Arc<Frame>> {
        let deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
        let mut hub = self.hub();
        loop {
            if let Some(frame) = &hub.frame {
                return Ok(frame.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "no frame was captured",
                ));
            }
            hub = self
                .changed
                .wait_timeout(hub, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    /// Block until `id` has an outstanding request that a newer frame (or a full
    /// refresh) can answer. `None` once the client is gone.
    fn next_job(&self, id: u64, seen: u64) -> Option<Job> {
        let mut hub = self.hub();
        loop {
            let frame = hub.frame.clone();
            let sequence = hub.sequence;
            let client = hub.clients.get_mut(&id).filter(|client| !client.closed)?;
            if let (true, Some(frame)) = (client.pending, frame)
                && (client.full || sequence != seen)
            {
                let full = std::mem::take(&mut client.full);
                // Claim this request before sending. The reader may receive the
                // next one while the socket write is still in progress.
                client.pending = false;
                return Some(Job {
                    frame,
                    sequence,
                    full,
                    format: client.format,
                    desktop_size: client.desktop_size,
                });
            }
            hub = self
                .changed
                .wait_timeout(hub, Duration::from_secs(1))
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    fn note_input(&self) {
        self.last_input_ms.store(
            (self.started.elapsed().as_millis() as u64).max(1),
            Ordering::Relaxed,
        );
    }
}

struct Job {
    frame: Arc<Frame>,
    sequence: u64,
    full: bool,
    format: PixelFormat,
    desktop_size: bool,
}

/// Bind `addr` and serve clients on background threads.
pub fn listen(addr: SocketAddr, shared: Arc<Shared>) -> io::Result<()> {
    serve_listener(TcpListener::bind(addr)?, shared)
}

/// Serve clients arriving on `listener` on background threads.
pub fn serve_listener(listener: TcpListener, shared: Arc<Shared>) -> io::Result<()> {
    *shared
        .listener
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(listener.try_clone()?);
    thread::Builder::new()
        .name("vnc-accept".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = shared.clone();
                let _ = thread::Builder::new()
                    .name("vnc-client".into())
                    .spawn(move || {
                        if let Err(error) = serve(stream, &shared)
                            && error.kind() != io::ErrorKind::UnexpectedEof
                        {
                            eprintln!("agx-gui: vnc client ended: {error}");
                        }
                    });
            }
        })?;
    Ok(())
}

fn serve(stream: TcpStream, shared: &Arc<Shared>) -> io::Result<()> {
    let Some(id) = shared.register() else {
        return Ok(());
    };
    let result = session(&stream, shared, id);
    shared.unregister(id);
    let _ = stream.shutdown(Shutdown::Both);
    result
}

/// Take over what the previous process left behind: the listening socket when it is
/// the one for `addr`, then each viewer, which is served without a new handshake and
/// gets a full picture as soon as one is captured. Returns whether the listener was
/// adopted; otherwise the caller binds its own.
pub fn adopt(handoff: &Handoff, addr: SocketAddr, shared: &Arc<Shared>) -> bool {
    let Some(listener) = handoff::adopt_listener(handoff.listener, addr) else {
        return false;
    };
    for viewer in &handoff.viewers {
        let Some(stream) = handoff::adopt_viewer(viewer.fd) else {
            continue;
        };
        let (shared, viewer) = (shared.clone(), viewer.clone());
        let _ = thread::Builder::new()
            .name("vnc-client".into())
            .spawn(move || {
                let Some(id) =
                    shared.register_with(viewer.format, viewer.desktop_size, viewer.size)
                else {
                    return;
                };
                let result = stream
                    .set_nodelay(true)
                    .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
                    .and_then(|()| stream_updates(&stream, &shared, id, viewer.size));
                shared.unregister(id);
                let _ = stream.shutdown(Shutdown::Both);
                if let Err(error) = result {
                    eprintln!("agx-gui: vnc viewer ended: {error}");
                }
            });
    }
    serve_listener(listener, shared.clone()).is_ok()
}

fn session(stream: &TcpStream, shared: &Arc<Shared>, id: u64) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    // Only the dimensions are needed for the handshake. Keeping the Arc in
    // this session's scope would retain its first capture until disconnect.
    let known_size = {
        let first = shared.first_frame()?;
        (first.width, first.height)
    };
    let (width, height) = (
        u16::try_from(known_size.0).map_err(io::Error::other)?,
        u16::try_from(known_size.1).map_err(io::Error::other)?,
    );
    rfb::handshake(&mut &*stream, width, height, SERVER_NAME)?;
    stream.set_read_timeout(None)?;
    shared.update_client(id, |client| client.pending = false);
    stream_updates(stream, shared, id, known_size)
}

/// Serve an established connection: read its input on one thread and send it updates
/// from this one, until either side ends. `known_size` is the picture the viewer has.
fn stream_updates(
    stream: &TcpStream,
    shared: &Arc<Shared>,
    id: u64,
    known_size: (usize, usize),
) -> io::Result<()> {
    let size = |frame: &Frame| -> io::Result<(u16, u16)> {
        Ok((
            u16::try_from(frame.width).map_err(io::Error::other)?,
            u16::try_from(frame.height).map_err(io::Error::other)?,
        ))
    };
    let kept = stream.try_clone()?;
    shared.update_client(id, |client| {
        client.stream = Some(kept);
        client.size = known_size;
    });
    let reader = stream.try_clone()?;
    let reader_shared = shared.clone();
    thread::Builder::new()
        .name("vnc-input".into())
        .spawn(move || {
            let _ = read_loop(reader, &reader_shared, id);
            reader_shared.update_client(id, |client| client.closed = true);
        })?;

    // Bound encoded output to 64 KiB instead of another full RGBA frame per viewer.
    let mut writer = BufWriter::with_capacity(64 * 1024, stream);
    let mut known_size = known_size;
    let mut sent: Option<Arc<Frame>> = None;
    let mut seen = 0;
    while let Some(job) = shared.next_job(id, seen) {
        seen = job.sequence;
        let resized = (job.frame.width, job.frame.height) != known_size;
        let area = match (&sent, job.full || resized) {
            (Some(previous), false) => changed_bounds(previous, &job.frame),
            _ => Some(job.frame.bounds()),
        };
        let Some(area) = area else {
            sent = Some(job.frame); // Nothing visible changed: keep the request pending.
            shared.update_client(id, |client| client.pending = true);
            continue;
        };
        let mut message = Vec::new();
        if resized {
            if !job.desktop_size {
                return Err(io::Error::other(
                    "window size changed and the client cannot resize",
                ));
            }
            known_size = (job.frame.width, job.frame.height);
            size(&job.frame)?;
            rfb::begin_update(&mut message, 2);
            rfb::rectangle_header(&mut message, job.frame.bounds(), ENCODING_DESKTOP_SIZE);
        } else {
            rfb::begin_update(&mut message, 1);
        }
        rfb::rectangle_header(&mut message, area, ENCODING_RAW);
        writer.write_all(&message)?;
        job.format.write_pixels(&job.frame, area, &mut writer)?;
        writer.flush()?;
        sent = Some(job.frame);
        shared.update_client(id, |client| {
            client.size = known_size;
        });
    }
    Ok(())
}

fn read_loop(stream: TcpStream, shared: &Arc<Shared>, id: u64) -> io::Result<()> {
    // Unbuffered on purpose: if the process is replaced mid-way, no half-read
    // message is lost to a read-ahead buffer.
    let mut reader = &stream;
    let mut buttons = 0;
    loop {
        match rfb::read_message(&mut reader)? {
            ClientMessage::SetPixelFormat(format) => {
                shared.update_client(id, |client| client.format = format)
            }
            ClientMessage::SetEncodings(encodings) => {
                shared.update_client(id, |client| {
                    client.desktop_size = encodings.contains(&ENCODING_DESKTOP_SIZE)
                });
            }
            ClientMessage::UpdateRequest { incremental } => shared.update_client(id, |client| {
                client.pending = true;
                client.full |= !incremental;
            }),
            ClientMessage::Key { down, keysym } => {
                shared.note_input();
                (shared.input)(Input::Key { down, keysym });
            }
            ClientMessage::Pointer { buttons: now, x, y } => {
                shared.note_input();
                for input in pointer_inputs(buttons, now, x, y) {
                    (shared.input)(input);
                }
                buttons = now;
            }
            ClientMessage::CutText => {}
        }
    }
}

#[cfg(test)]
mod tests;
