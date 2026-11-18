//! Who is asking. Approvals and pairing answers are for the person at the desktop; an agent or
//! the CLI running as the same account is held to the approval queue instead.
//!
//! This guards against unattended action by an agent or script that goes through the socket.
//! It is not a boundary against a hostile program running as the same account: such a program
//! could already drive the desktop itself.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    Desktop,
    Other,
}

/// The only executable trusted to answer on the person's behalf.
const DESKTOP: &str = "agx-gui";

/// Classify by the connecting process's executable path as the kernel reports it. A replaced
/// binary (after a desktop update) is reported with a trailing " (deleted)".
pub fn classify(exe: &str) -> Peer {
    let path = exe.strip_suffix(" (deleted)").unwrap_or(exe);
    match std::path::Path::new(path).file_name().and_then(|name| name.to_str()) {
        Some(DESKTOP) => Peer::Desktop,
        _ => Peer::Other,
    }
}

pub fn of_pid(pid: Option<i32>) -> Peer {
    pid.and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
        .map_or(Peer::Other, |exe| classify(&exe.to_string_lossy()))
}
