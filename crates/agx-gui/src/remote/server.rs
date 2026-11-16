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
    io::{self, Write},
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
        self.hub().clients.remove(&id);
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
    let first = shared.first_frame()?;
    let (width, height) = (
        u16::try_from(first.width).map_err(io::Error::other)?,
        u16::try_from(first.height).map_err(io::Error::other)?,
    );
    rfb::handshake(&mut &*stream, width, height, SERVER_NAME)?;
    stream.set_read_timeout(None)?;
    shared.update_client(id, |client| client.pending = false);
    stream_updates(stream, shared, id, (first.width, first.height))
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

    let mut writer = stream;
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
        job.format.encode(&job.frame, area, &mut message);
        writer.write_all(&message)?;
        sent = Some(job.frame);
        shared.update_client(id, |client| {
            client.pending = false;
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
mod tests {
    use super::*;
    use std::io::Read;

    fn frame(width: usize, height: usize, value: u8) -> Frame {
        Frame {
            width,
            height,
            rgba: vec![value; width * height * 4],
        }
    }

    struct Client(TcpStream);

    impl Client {
        fn connect(addr: SocketAddr) -> Self {
            let stream = TcpStream::connect(addr).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            Self(stream)
        }

        fn read<const N: usize>(&mut self) -> [u8; N] {
            let mut bytes = [0; N];
            self.0.read_exact(&mut bytes).unwrap();
            bytes
        }

        fn handshake(&mut self) -> (u16, u16) {
            assert_eq!(&self.read::<12>(), b"RFB 003.008\n");
            self.0.write_all(b"RFB 003.008\n").unwrap();
            assert_eq!(self.read::<2>(), [1, 1]);
            self.0.write_all(&[1]).unwrap();
            assert_eq!(self.read::<4>(), [0; 4]);
            self.0.write_all(&[1]).unwrap(); // ClientInit: shared
            let [w0, w1, h0, h1] = self.read::<4>();
            self.read::<16>(); // pixel format
            let [.., n] = self.read::<4>();
            let mut name = vec![0; n as usize];
            self.0.read_exact(&mut name).unwrap();
            assert_eq!(name, SERVER_NAME.as_bytes());
            (u16::from_be_bytes([w0, w1]), u16::from_be_bytes([h0, h1]))
        }

        fn request(&mut self, incremental: bool) {
            self.0
                .write_all(&[3, u8::from(incremental), 0, 0, 0, 0, 0, 8, 0, 4])
                .unwrap();
        }

        /// One update: (rectangle x, y, width, height, pixel bytes).
        fn update(&mut self) -> (u16, u16, u16, u16, Vec<u8>) {
            let [kind, _, rects @ ..] = self.read::<4>();
            assert_eq!((kind, rects), (0, [0, 1]));
            let header = self.read::<12>();
            let field = |i: usize| u16::from_be_bytes([header[i * 2], header[i * 2 + 1]]);
            assert_eq!(&header[8..], &ENCODING_RAW.to_be_bytes());
            let mut pixels = vec![0; usize::from(field(2)) * usize::from(field(3)) * 4];
            self.0.read_exact(&mut pixels).unwrap();
            (field(0), field(1), field(2), field(3), pixels)
        }
    }

    #[test]
    fn a_client_sees_the_frame_then_only_what_changes_and_can_send_input() {
        let (sender, received) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let shared = Shared::new(move |input| sender.lock().unwrap().send(input).unwrap());
        shared.publish(frame(8, 4, 10));
        // Port 0 cannot be learned back through `listen`, so bind first and reuse the port.
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        listen(addr, shared.clone()).unwrap();

        let mut client = Client::connect(addr);
        assert_eq!(client.handshake(), (8, 4));
        client.request(false);
        let (x, y, w, h, pixels) = client.update();
        assert_eq!((x, y, w, h), (0, 0, 8, 4));
        assert!(pixels.chunks(4).all(|pixel| pixel == [10, 10, 10, 0]));

        // Change one pixel at (3, 2); an incremental request gets just that rectangle.
        client.request(true);
        let mut next = frame(8, 4, 10);
        next.rgba[(2 * 8 + 3) * 4..][..3].copy_from_slice(&[1, 2, 3]);
        thread::sleep(Duration::from_millis(100)); // the request is pending before the frame lands
        shared.publish(next);
        assert_eq!(client.update(), (3, 2, 1, 1, vec![3, 2, 1, 0]));

        client.0.write_all(&[4, 1, 0, 0, 0, 0, 0, 0x61]).unwrap(); // key 'a' down
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Input::Key {
                down: true,
                keysym: 0x61
            }
        );
        assert!(shared.input_is_recent());
    }

    #[test]
    fn a_new_server_adopts_the_listener_and_viewers_of_the_one_it_replaces() {
        use std::os::fd::{AsRawFd, RawFd};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let viewer = TcpStream::connect(addr).unwrap();
        let (served, _) = listener.accept().unwrap();
        // The previous process would pass its descriptors through exec; duplicates stand in.
        let dup = |fd: RawFd| unsafe { libc::dup(fd) };
        let handoff = Handoff {
            listener: dup(listener.as_raw_fd()),
            viewers: vec![Viewer {
                fd: dup(served.as_raw_fd()),
                size: (8, 4),
                desktop_size: true,
                format: PixelFormat::NATIVE,
            }],
        };
        let (sender, received) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let shared = Shared::new(move |input| sender.lock().unwrap().send(input).unwrap());
        shared.publish(frame(8, 4, 10));
        assert!(adopt(&handoff, addr, &shared));

        // No new handshake: the viewer is sent a full picture at once.
        let mut viewer = Client(viewer);
        viewer
            .0
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (x, y, w, h, pixels) = viewer.update();
        assert_eq!((x, y, w, h), (0, 0, 8, 4));
        assert!(pixels.chunks(4).all(|pixel| pixel == [10, 10, 10, 0]));

        // Its input reaches the new server.
        viewer.0.write_all(&[4, 1, 0, 0, 0, 0, 0, 0x62]).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Input::Key {
                down: true,
                keysym: 0x62
            }
        );

        // A size change is announced with DesktopSize because the viewer said it supports it.
        viewer.request(true);
        thread::sleep(Duration::from_millis(100));
        shared.publish(frame(16, 6, 20));
        assert_eq!(viewer.read::<4>(), [0, 0, 0, 2]);
        let size = viewer.read::<12>();
        assert_eq!(&size[..8], &[0, 0, 0, 0, 0, 16, 0, 6]);
        assert_eq!(&size[8..], &ENCODING_DESKTOP_SIZE.to_be_bytes());
        viewer.read::<12>(); // the raw rectangle header
        let mut picture = vec![0; 16 * 6 * 4];
        viewer.0.read_exact(&mut picture).unwrap();

        // The adopted listener keeps accepting new viewers.
        assert_eq!(Client::connect(addr).handshake(), (16, 6));
    }

    #[test]
    fn a_handoff_for_another_address_is_not_adopted() {
        use std::os::fd::AsRawFd;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let handoff = Handoff {
            listener: unsafe { libc::dup(listener.as_raw_fd()) },
            viewers: vec![],
        };
        let shared = Shared::new(|_| {});
        assert!(!adopt(&handoff, "127.0.0.1:1".parse().unwrap(), &shared));
    }

    #[test]
    fn the_handoff_lists_only_connected_viewers() {
        let shared = Shared::new(|_| {});
        assert!(shared.handoff().is_none(), "no listener yet");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        *shared.listener.lock().unwrap() = Some(listener.try_clone().unwrap());
        let id = shared.register().unwrap();
        assert!(
            shared.handoff().unwrap().viewers.is_empty(),
            "still handshaking"
        );
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        shared.update_client(id, |client| {
            client.stream = Some(stream);
            client.size = (8, 4);
        });
        let handoff = shared.handoff().unwrap();
        assert_eq!(handoff.viewers.len(), 1);
        assert_eq!(handoff.viewers[0].size, (8, 4));
        shared.update_client(id, |client| client.closed = true);
        assert!(shared.handoff().unwrap().viewers.is_empty());
    }

    #[test]
    fn identical_frames_do_not_wake_clients() {
        let shared = Shared::new(|_| {});
        shared.publish(frame(2, 2, 1));
        let sequence = shared.hub().sequence;
        shared.publish(frame(2, 2, 1));
        assert_eq!(shared.hub().sequence, sequence);
        shared.publish(frame(2, 2, 2));
        assert_eq!(shared.hub().sequence, sequence + 1);
    }

    #[test]
    fn the_number_of_clients_is_capped() {
        let shared = Shared::new(|_| {});
        let ids: Vec<_> = (0..MAX_CLIENTS)
            .map(|_| shared.register().unwrap())
            .collect();
        assert!(shared.register().is_none());
        shared.unregister(ids[0]);
        assert!(shared.register().is_some());
    }
}
