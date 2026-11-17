//! Prepare desktop device permissions before forwarding kernel arrival/removal events.
use std::{
    ffi::CString,
    fs, io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    },
    path::Path,
    time::{Duration, Instant},
};

fn field<'a>(message: &'a [u8], key: &str) -> Option<&'a str> {
    message.split(|byte| *byte == 0).find_map(|part| {
        let text = std::str::from_utf8(part).ok()?;
        let (name, value) = text.split_once('=')?;
        (name == key).then_some(value)
    })
}
fn input_event(message: &[u8]) -> Option<(&str, &str)> {
    if message.last() != Some(&0) || field(message, "SUBSYSTEM")? != "input" {
        return None;
    }
    let action = field(message, "ACTION")?;
    if !matches!(action, "add" | "remove" | "change") {
        return None;
    }
    let name = field(message, "DEVNAME")?;
    let number = name.strip_prefix("input/event")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let path = field(message, "DEVPATH")?;
    if !path.starts_with("/devices/") || path.split('/').any(|part| part == "..") {
        return None;
    }
    let sequence = field(message, "SEQNUM")?;
    if sequence.is_empty() || !sequence.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((action, name))
}
fn allow_device(path: &Path, group: libc::gid_t) -> io::Result<bool> {
    let info = match fs::symlink_metadata(path) {
        Ok(info) => info,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    // These are root-owned devtmpfs directories; never follow a symlink.
    if !info.file_type().is_char_device() {
        return Ok(false);
    }
    if info.gid() != group {
        let name = CString::new(path.as_os_str().as_encoded_bytes())?;
        if unsafe { libc::chown(name.as_ptr(), !0, group) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if info.mode() & 0o7777 != 0o660 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
    }
    Ok(true)
}
fn refresh_access(group: libc::gid_t) -> io::Result<()> {
    for directory in ["/dev/dri", "/dev/input"] {
        match fs::read_dir(directory) {
            Ok(entries) => {
                for entry in entries {
                    allow_device(&entry?.path(), group)?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    for path in ["/dev/fb0", "/dev/console"] {
        allow_device(Path::new(path), group)?;
    }
    Ok(())
}
fn address(group: u32) -> libc::sockaddr_nl {
    let mut address: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    address.nl_family = libc::AF_NETLINK as u16;
    address.nl_groups = group;
    address
}
fn socket() -> io::Result<OwnedFd> {
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_KOBJECT_UEVENT,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let address = address(1); // Only kernel device lifecycle notifications, never keystrokes.
    if unsafe {
        libc::bind(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_nl).cast(),
            std::mem::size_of_val(&address) as _,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}
fn forward(fd: &OwnedFd, message: &[u8]) -> io::Result<()> {
    // libudev-zero deliberately listens on group 4 after device permissions are ready.
    let target = address(4);
    if unsafe {
        libc::sendto(
            fd.as_raw_fd(),
            message.as_ptr().cast(),
            message.len(),
            0,
            (&target as *const libc::sockaddr_nl).cast(),
            std::mem::size_of_val(&target) as _,
        )
    } < 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ECONNREFUSED) {
            return Err(error);
        }
    }
    Ok(())
}
pub fn run() -> io::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(io::Error::other("Device setup requires root"));
    }
    let group = unsafe { libc::getgrnam(c"agx-admin".as_ptr()) };
    if group.is_null() {
        return Err(io::Error::other("Missing agx-admin group"));
    }
    let group = unsafe { (*group).gr_gid };
    // Listen before scanning: devices arriving during the initial scan must not be lost.
    let fd = socket()?;
    let mut next_scan = Instant::now();
    let mut bytes = [0u8; 8192];
    loop {
        if Instant::now() >= next_scan {
            refresh_access(group)?;
            next_scan = Instant::now() + Duration::from_secs(1);
        }
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout = next_scan
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(1000) as i32;
        let result = unsafe { libc::poll(&mut poll, 1, timeout) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if result == 0 {
            continue;
        }
        let mut sender = address(0);
        let mut length = std::mem::size_of_val(&sender) as libc::socklen_t;
        let size = unsafe {
            libc::recvfrom(
                fd.as_raw_fd(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                libc::MSG_TRUNC,
                (&mut sender as *mut libc::sockaddr_nl).cast(),
                &mut length,
            )
        };
        if size < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if size as usize > bytes.len() || sender.nl_pid != 0 || sender.nl_groups != 1 {
            continue;
        }
        let message = &bytes[..size as usize];
        let Some((action, name)) = input_event(message) else {
            continue;
        };
        if action != "remove" {
            let path = Path::new("/dev").join(name);
            let deadline = Instant::now() + Duration::from_millis(100);
            while !allow_device(&path, group)? && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            if !allow_device(&path, group)? {
                continue;
            }
        }
        forward(&fd, message)?;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const ADD: &[u8] = b"add@/devices/virtual/input/input20/event8\0ACTION=add\0DEVPATH=/devices/virtual/input/input20/event8\0SUBSYSTEM=input\0DEVNAME=input/event8\0SEQNUM=222\0";
    #[test]
    fn accepts_kernel_input_lifecycle_messages() {
        assert_eq!(input_event(ADD), Some(("add", "input/event8")));
        let remove = String::from_utf8(ADD.to_vec())
            .unwrap()
            .replace("ACTION=add", "ACTION=remove");
        assert_eq!(
            input_event(remove.as_bytes()),
            Some(("remove", "input/event8"))
        );
    }
    #[test]
    fn rejects_unrelated_devices_and_unsafe_or_incomplete_records() {
        let valid = String::from_utf8(ADD.to_vec()).unwrap();
        for invalid in [
            valid.replace("SUBSYSTEM=input", "SUBSYSTEM=block"),
            valid.replace("input/event8", "input/../../etc/shadow"),
            valid.replace("SEQNUM=222", "SEQNUM=x"),
            valid.replace("DEVPATH=/devices/", "DEVPATH=/devices/../"),
            valid.replace("ACTION=add", "ACTION=bind"),
        ] {
            assert!(input_event(invalid.as_bytes()).is_none());
        }
        assert!(input_event(&ADD[..ADD.len() - 1]).is_none());
    }
    #[test]
    fn device_access_does_not_follow_symlinks_or_change_regular_files() {
        let path = std::env::temp_dir().join(format!("agx-input-access-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let target = path.join("file");
        fs::write(&target, b"unchanged").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let link = path.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(!allow_device(&target, 0).unwrap());
        assert!(!allow_device(&link, 0).unwrap());
        assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o600);
        fs::remove_dir_all(path).unwrap();
    }
}
