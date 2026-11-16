//! Keeps VNC viewers connected while the root login screen turns into the user's
//! desktop. The login process is replaced (exec) by a new one, which would drop every
//! socket. Instead the login screen lets the listening socket and each connected viewer
//! survive the exec and describes them in `AGX_GUI_VNC_HANDOFF`; the desktop adopts
//! them and carries on from where the login screen stopped, with no new handshake.
//!
//! Anything unexpected makes the desktop ignore the handoff and bind its own listener.
use super::rfb::PixelFormat;
use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    os::fd::{FromRawFd, RawFd},
};

pub const ENV: &str = "AGX_GUI_VNC_HANDOFF";
/// Plain comma-separated descriptor numbers, so the launcher can close its copies.
pub const FDS_ENV: &str = "AGX_GUI_VNC_FDS";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewer {
    pub fd: RawFd,
    /// Size of the picture the viewer currently has.
    pub size: (usize, usize),
    pub desktop_size: bool,
    pub extended_desktop_size: bool,
    pub format: PixelFormat,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Handoff {
    pub listener: RawFd,
    pub viewers: Vec<Viewer>,
}

impl Handoff {
    pub fn fds(&self) -> Vec<RawFd> {
        std::iter::once(self.listener)
            .chain(self.viewers.iter().map(|viewer| viewer.fd))
            .collect()
    }

    pub fn fds_env(&self) -> String {
        let fds: Vec<_> = self.fds().iter().map(ToString::to_string).collect();
        fds.join(",")
    }

    /// `<listener>;<fd>,<width>,<height>,<resize flags>,<pixel format hex>;...`
    /// Bit 0 is DesktopSize, bit 1 is ExtendedDesktopSize; older 0/1 values still work.
    pub fn encode(&self) -> String {
        let mut text = self.listener.to_string();
        for viewer in &self.viewers {
            let format: String = viewer
                .format
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            text += &format!(
                ";{},{},{},{},{format}",
                viewer.fd,
                viewer.size.0,
                viewer.size.1,
                u8::from(viewer.desktop_size) | (u8::from(viewer.extended_desktop_size) << 1)
            );
        }
        text
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(';');
        let listener = descriptor(parts.next()?)?;
        let viewers = parts.map(parse_viewer).collect::<Option<Vec<_>>>()?;
        let handoff = Self { listener, viewers };
        let mut fds = handoff.fds();
        fds.sort_unstable();
        fds.dedup();
        (fds.len() == handoff.viewers.len() + 1).then_some(handoff)
    }
}

fn descriptor(text: &str) -> Option<RawFd> {
    text.parse().ok().filter(|fd| *fd > 2)
}

fn parse_viewer(text: &str) -> Option<Viewer> {
    let mut fields = text.split(',');
    let fd = descriptor(fields.next()?)?;
    let width = fields.next()?.parse().ok()?;
    let height = fields.next()?.parse().ok()?;
    let flags: u8 = match fields.next()? {
        "0" => 0,
        "1" => 1,
        "2" => 2,
        "3" => 3,
        _ => return None,
    };
    let hex = fields.next()?;
    if fields.next().is_some() || hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (byte, pair) in bytes.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(Viewer {
        fd,
        size: (width, height),
        desktop_size: flags & 1 != 0,
        extended_desktop_size: flags & 2 != 0,
        format: PixelFormat::parse(&bytes).ok()?,
    })
}

fn is_socket(fd: RawFd) -> bool {
    // SAFETY: fstat only reads the descriptor; an invalid one returns an error.
    let mut info: libc::stat = unsafe { std::mem::zeroed() };
    unsafe { libc::fstat(fd, &mut info) == 0 && info.st_mode & libc::S_IFMT == libc::S_IFSOCK }
}

fn is_listening(fd: RawFd) -> bool {
    let mut value: libc::c_int = 0;
    let mut length = size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: the pointers refer to live locals of the sizes passed.
    unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ACCEPTCONN,
            (&raw mut value).cast(),
            &mut length,
        ) == 0
            && value != 0
    }
}

/// Take ownership of the inherited listening socket, if it really is one bound to `addr`.
pub fn adopt_listener(fd: RawFd, addr: SocketAddr) -> Option<TcpListener> {
    if !is_socket(fd) || !is_listening(fd) {
        return None;
    }
    // SAFETY: a listening socket we were handed through the exec, adopted exactly once.
    let listener = unsafe { TcpListener::from_raw_fd(fd) };
    (listener.local_addr().ok()? == addr).then_some(listener)
}

/// Take ownership of an inherited viewer connection, if it is a loopback TCP socket.
pub fn adopt_viewer(fd: RawFd) -> Option<TcpStream> {
    if !is_socket(fd) || is_listening(fd) {
        return None;
    }
    // SAFETY: a connected socket we were handed through the exec, adopted exactly once.
    let stream = unsafe { TcpStream::from_raw_fd(fd) };
    stream
        .peer_addr()
        .ok()?
        .ip()
        .is_loopback()
        .then_some(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer(fd: RawFd) -> Viewer {
        Viewer {
            fd,
            size: (1920, 1080),
            desktop_size: true,
            extended_desktop_size: true,
            format: PixelFormat {
                bits_per_pixel: 16,
                depth: 16,
                big_endian: false,
                max: [31, 63, 31],
                shift: [11, 5, 0],
            },
        }
    }

    #[test]
    fn a_handoff_survives_the_environment_round_trip() {
        let handoff = Handoff {
            listener: 3,
            viewers: vec![viewer(4), viewer(7)],
        };
        assert_eq!(Handoff::parse(&handoff.encode()), Some(handoff));
        assert_eq!(
            Handoff {
                listener: 3,
                viewers: vec![viewer(4)]
            }
            .fds_env(),
            "3,4"
        );
        let empty = Handoff {
            listener: 5,
            viewers: vec![],
        };
        assert_eq!(Handoff::parse(&empty.encode()), Some(empty));
    }

    #[test]
    fn malformed_or_dangerous_handoffs_are_ignored() {
        let good = Handoff {
            listener: 3,
            viewers: vec![viewer(4)],
        }
        .encode();
        assert!(Handoff::parse(&good).is_some());
        for bad in [
            "",
            "x",
            "0",                                          // stdin
            "2",                                          // stderr
            "-1",                                         // not a descriptor
            "3;4",                                        // truncated viewer
            "3;4,1,1,1,00",                               // short pixel format
            "3;3,1,1,1,00000000000000000000000000000000", // listener reused as a viewer
            &good.replace(",3,", ",4,"),                  // desktop-size flags out of range
            &format!("{good};5,1,1,0,{}", "zz".repeat(16)),
        ] {
            assert_eq!(Handoff::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn only_real_sockets_with_the_right_role_are_adopted() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (served, _) = listener.accept().unwrap();
        let wrong: SocketAddr = "127.0.0.1:1".parse().unwrap();
        use std::os::fd::AsRawFd;
        assert!(!is_socket(-1));
        assert!(is_listening(listener.as_raw_fd()));
        assert!(!is_listening(client.as_raw_fd()));
        assert!(is_socket(client.as_raw_fd()));
        // Wrong roles and wrong addresses are rejected before ownership is taken.
        assert!(adopt_viewer(listener.as_raw_fd()).is_none());
        assert!(adopt_listener(client.as_raw_fd(), addr).is_none());
        let regular = std::fs::File::open("/dev/null").unwrap();
        assert!(!is_socket(regular.as_raw_fd()));
        // Adopt duplicates so the originals stay owned by their `TcpStream`s.
        let dup = |fd: RawFd| unsafe { libc::dup(fd) };
        let adopted = adopt_viewer(dup(served.as_raw_fd())).expect("loopback viewer");
        assert_eq!(adopted.peer_addr().unwrap(), client.local_addr().unwrap());
        assert!(adopt_listener(dup(listener.as_raw_fd()), wrong).is_none());
        assert!(adopt_listener(dup(listener.as_raw_fd()), addr).is_some());
    }
}
