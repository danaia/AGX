//! First-boot setup on the USB gadget only. The Unix service remains independent.
mod claim;
mod http;

use std::{
    io,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const ADDRESS: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 2);
const PEER: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 1);
const PAGE: &[u8] = include_bytes!("setup.html");
const SCRIPT: &[u8] = include_bytes!("setup.js");
const STYLES: &[u8] = include_bytes!("setup.css");
const BRAND: &[u8] = include_bytes!("../../../../brand/brand-bug.svg");

pub(crate) fn start(stop: Arc<AtomicBool>) {
    if !Path::new(claim::ENABLED).is_file() {
        return;
    }
    if let Err(error) = thread::Builder::new()
        .name("usb-setup".into())
        .spawn(move || run(&stop))
    {
        eprintln!("ANGI USB setup could not start: {error}");
    }
}

fn bind_usb() -> io::Result<TcpListener> {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let raw = fd.as_raw_fd();
    let device = c"usb0".to_bytes_with_nul();
    let local = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: 80u16.to_be(),
        sin_addr: libc::in_addr {
            s_addr: u32::from_ne_bytes(ADDRESS.octets()),
        },
        sin_zero: [0; 8],
    };
    if unsafe {
        libc::setsockopt(
            raw,
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            device.as_ptr().cast(),
            device.len() as libc::socklen_t,
        )
    } != 0
        || unsafe {
            libc::bind(
                raw,
                (&raw const local).cast(),
                size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        } != 0
        || unsafe { libc::listen(raw, 4) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    let listener = TcpListener::from(fd);
    listener.set_nonblocking(true)?;
    Ok(listener)
}

struct Helper {
    command: &'static str,
    arguments: &'static [&'static str],
    child: Option<Child>,
}

impl Helper {
    fn maintain(&mut self) {
        if let Some(child) = &mut self.child
            && matches!(child.try_wait(), Ok(None))
        {
            return;
        }
        let parent = std::process::id();
        let mut command = Command::new(self.command);
        command
            .args(self.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Avoid leaving a DHCP or discovery process behind when netd is restarted.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() as u32 != parent {
                    return Err(io::Error::other("network service stopped"));
                }
                Ok(())
            });
        }
        self.child = command.spawn().ok();
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn run(stop: &AtomicBool) {
    let mut dhcp = Helper {
        command: "/usr/sbin/udhcpd",
        arguments: &["-f", "/etc/agx/dhcp-usb.conf"],
        child: None,
    };
    let mut discovery = Some(Helper {
        command: "/usr/sbin/avahi-daemon",
        arguments: &[],
        child: None,
    });
    let mut listener = None;
    let mut fingerprint = None;
    let mut next_retry = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next_retry {
            next_retry = Instant::now() + Duration::from_secs(3);
            dhcp.maintain();
            if matches!(claim::unclaimed(), Ok(true)) {
                if fingerprint.is_none() {
                    fingerprint = claim::host_fingerprint().ok();
                }
                if fingerprint.is_some() && listener.is_none() {
                    listener = bind_usb().ok();
                }
                if listener.is_some()
                    && let Some(discovery) = &mut discovery
                {
                    discovery.maintain();
                }
            } else {
                listener = None;
                discovery = None;
            }
        }
        if let (Some(socket), Some(fingerprint)) = (&listener, &fingerprint)
            && let Ok((mut connection, peer)) = socket.accept()
            && matches!(peer, SocketAddr::V4(peer) if *peer.ip() == PEER)
            && matches!(claim::unclaimed(), Ok(true))
        {
            let _ = serve(&mut connection, fingerprint, stop);
            if matches!(claim::unclaimed(), Ok(false)) {
                listener = None;
                discovery = None;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn serve(connection: &mut TcpStream, fingerprint: &str, stop: &AtomicBool) -> io::Result<()> {
    match http::read(connection) {
        Ok(http::Request::Page) => {
            http::send(connection, "200 OK", "text/html; charset=utf-8", PAGE)
        }
        Ok(http::Request::Script) => http::send(connection, "200 OK", "text/javascript", SCRIPT),
        Ok(http::Request::Styles) => http::send(connection, "200 OK", "text/css", STYLES),
        Ok(http::Request::Brand) => http::send(connection, "200 OK", "image/svg+xml", BRAND),
        Ok(http::Request::Scan(body)) => {
            let (status, response) = match claim::scan(&body, stop) {
                Ok(networks) => (
                    "200 OK",
                    serde_json::json!({ "ok": true, "networks": networks }),
                ),
                Err(message) => (
                    "422 Unprocessable Content",
                    serde_json::json!({ "ok": false, "message": message }),
                ),
            };
            http::send(
                connection,
                status,
                "application/json",
                &serde_json::to_vec(&response)?,
            )
        }
        Ok(http::Request::Claim(body)) => {
            let (status, ok, message) = match claim::submit(&body, fingerprint, stop) {
                Ok(message) => ("200 OK", true, message),
                Err(message) => ("422 Unprocessable Content", false, message),
            };
            let response =
                serde_json::to_vec(&serde_json::json!({ "ok": ok, "message": message }))?;
            http::send(connection, status, "application/json", &response)
        }
        Ok(http::Request::NotFound) => {
            http::send(connection, "404 Not Found", "text/plain", b"Not found")
        }
        Err(_) => http::send(
            connection,
            "400 Bad Request",
            "text/plain",
            b"Invalid setup request",
        ),
    }
}
