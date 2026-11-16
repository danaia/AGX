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

#[test]
fn an_update_requested_while_sending_a_frame_is_preserved() {
    let (sender, received) = std::sync::mpsc::channel();
    let shared = Shared::new(move |input| sender.send(input).unwrap());
    shared.publish(frame(1024, 512, 10));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let serving = shared.clone();
    let worker = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        // Force the picture to remain in flight until the client drains it.
        let bytes: libc::c_int = 4096;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&bytes as *const libc::c_int).cast(),
                    std::mem::size_of_val(&bytes) as libc::socklen_t,
                )
            },
            0
        );
        serve(stream, &serving).unwrap();
    });
    let mut client = Client::connect(addr);
    assert_eq!(client.handshake(), (1024, 512));
    client.request(false);
    assert_eq!(client.read::<4>(), [0, 0, 0, 1]);
    client.read::<12>();
    client.request(false);
    client.0.write_all(&[4, 1, 0, 0, 0, 0, 0, 0x61]).unwrap();
    // Input follows the request on the same stream: this proves the reader has
    // processed the second request before we let the first write finish.
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut pixels = vec![0; 1024 * 512 * 4];
    client.0.read_exact(&mut pixels).unwrap();
    let (_, _, w, h, _) = client.update();
    assert_eq!((w, h), (1024, 512));
    drop(client);
    worker.join().unwrap();
}

#[test]
fn the_last_viewer_releases_the_retained_capture() {
    let shared = Shared::new(|_| {});
    let first = shared.register().unwrap();
    let second = shared.register().unwrap();
    shared.publish(frame(8, 4, 10));
    let captured = Arc::downgrade(shared.hub().frame.as_ref().unwrap());
    shared.unregister(first);
    assert!(captured.upgrade().is_some());
    shared.unregister(second);
    assert!(captured.upgrade().is_none());
    assert!(shared.hub().frame.is_none());
}

#[test]
fn the_handshake_does_not_retain_an_obsolete_capture_for_the_session() {
    let shared = Shared::new(|_| {});
    shared.publish(frame(8, 4, 10));
    let original = Arc::downgrade(shared.hub().frame.as_ref().unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let serving = shared.clone();
    let worker = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        serve(stream, &serving).unwrap();
    });
    let mut client = Client::connect(addr);
    assert_eq!(client.handshake(), (8, 4));
    shared.publish(frame(8, 4, 20));
    client.request(false);
    let (_, _, _, _, pixels) = client.update();
    assert!(pixels.chunks(4).all(|pixel| pixel == [20, 20, 20, 0]));
    assert!(
        original.upgrade().is_none(),
        "the session needs dimensions, not the old pixels"
    );
    drop(client);
    worker.join().unwrap();
}

#[test]
fn an_idle_poll_releases_a_capture_finished_after_the_last_viewer_left() {
    let shared = Shared::new(|_| {});
    let id = shared.register().unwrap();
    assert!(shared.wants_frame()); // The UI starts capturing for this viewer.
    shared.unregister(id);
    shared.publish(frame(8, 4, 10)); // Its capture completes after the disconnect.
    let late = Arc::downgrade(shared.hub().frame.as_ref().unwrap());
    assert!(!shared.wants_frame());
    assert!(late.upgrade().is_none());
    // A new viewer still causes a fresh capture rather than using an old picture.
    shared.register().unwrap();
    assert!(shared.wants_frame());
}
