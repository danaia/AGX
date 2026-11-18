//! Process snapshots and guarded termination for the memory overlay.
use crate::{AppWindow, MemoryProcesses, ProcessRow, system_ui::bytes};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::HashSet,
    fs, io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    pid: i32,
    start: u64,
}

struct Process {
    id: Identity,
    parent: i32,
    uid: u32,
    name: String,
    rss: u64,
    kernel: bool,
    exited: bool,
    simple_utility: bool,
}

fn read(pid: i32, page_size: u64) -> Option<Process> {
    let path = format!("/proc/{pid}");
    let stat = fs::read_to_string(format!("{path}/stat")).ok()?;
    let (_, tail) = stat.rsplit_once(") ")?;
    let fields: Vec<_> = tail.split_ascii_whitespace().collect();
    let status = fs::read_to_string(format!("{path}/status")).ok()?;
    let uid = status.lines().find_map(|line| line.strip_prefix("Uid:"))?;
    let ids: Vec<u32> = uid
        .split_ascii_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let real = *ids.first()?;
    let name = stat.split_once('(')?.1.rsplit_once(") ")?.0;
    let simple_utility = matches!(name, "sleep" | "yes")
        && fs::read_link(format!("{path}/exe")).is_ok_and(|exe| {
            matches!(
                exe.to_str(),
                Some(
                    "/usr/bin/sleep"
                        | "/bin/sleep"
                        | "/usr/bin/yes"
                        | "/bin/yes"
                        | "/bin/busybox"
                        | "/usr/bin/busybox"
                )
            )
        });
    Some(Process {
        id: Identity {
            pid,
            start: fields.get(19)?.parse().ok()?,
        },
        parent: fields.get(1)?.parse().ok()?,
        uid: if ids.len() == 4 && ids.iter().all(|id| *id == real) {
            real
        } else {
            0
        },
        name: name
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect(),
        rss: fields.get(21)?.parse::<i64>().ok()?.max(0) as u64 * page_size,
        kernel: fields.get(6)?.parse::<u64>().ok()? & 0x0020_0000 != 0,
        simple_utility,
        exited: matches!(*fields.first()?, "Z" | "X" | "x"),
    })
}

#[derive(PartialEq, Eq)]
enum Risk {
    Protected(&'static str),
    Higher(&'static str),
    Lower,
}

impl Risk {
    fn label(&self) -> &'static str {
        match self {
            Self::Protected(_) => "Protected",
            Self::Higher(_) => "Risky",
            Self::Lower => "Lower risk",
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            Self::Protected(reason) | Self::Higher(reason) => reason,
            Self::Lower => "A simple utility with no child processes. Its current task will stop.",
        }
    }
}

struct Snapshot {
    processes: Vec<Process>,
    session: HashSet<i32>,
    parents: HashSet<i32>,
}

impl Snapshot {
    fn read() -> io::Result<Self> {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        let processes: Vec<_> = fs::read_dir("/proc")?
            .flatten()
            .filter_map(|entry| read(entry.file_name().to_str()?.parse().ok()?, page_size))
            .collect();
        let parents = processes.iter().map(|process| process.parent).collect();
        let mut session = HashSet::new();
        let mut pid = std::process::id() as i32;
        while pid > 0 && session.insert(pid) {
            let Some(process) = processes.iter().find(|process| process.id.pid == pid) else {
                break;
            };
            pid = process.parent;
        }
        Ok(Self {
            processes,
            session,
            parents,
        })
    }

    fn risk(&self, process: &Process) -> Risk {
        let uid = unsafe { libc::geteuid() };
        if self.session.contains(&process.id.pid)
            || process.id.pid <= 1
            || process.name == "agx-gui"
        {
            Risk::Protected("Part of the desktop or its startup chain.")
        } else if process.kernel || process.uid == 0 || process.uid != uid {
            Risk::Protected("System process or a process owned by another account.")
        } else if process.exited {
            Risk::Protected("This process has already exited.")
        } else if self.parents.contains(&process.id.pid) {
            Risk::Higher("This process has children. Ending it can interrupt their work.")
        } else if process.simple_utility {
            Risk::Lower
        } else {
            Risk::Higher("Ending this process may interrupt an app, a session, or unsaved work.")
        }
    }
}

fn terminate(id: Identity, force: bool) -> io::Result<()> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, id.pid, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Re-read ownership and identity after opening the descriptor. A reused PID
    // cannot redirect the signal once the descriptor has been acquired.
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    let snapshot = Snapshot::read()?;
    let process = snapshot
        .processes
        .iter()
        .find(|process| process.id == id)
        .ok_or_else(|| io::Error::other("That process has changed or exited."))?;
    if let Risk::Protected(reason) = snapshot.risk(process) {
        return Err(io::Error::other(reason));
    }
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

enum Request {
    Scan,
    End(Identity, bool),
}

struct Update {
    snapshot: io::Result<Snapshot>,
    message: Option<String>,
}

#[derive(Default)]
struct State {
    snapshot: Option<Snapshot>,
    selected: Option<Identity>,
    confirmed: Option<Identity>,
    ascending: bool,
    waiting: bool,
}

fn publish(ui: &AppWindow, state: &State, model: &VecModel<ProcessRow>) {
    let Some(snapshot) = &state.snapshot else {
        return;
    };
    let view = ui.global::<MemoryProcesses>();
    let mut ordered: Vec<_> = snapshot.processes.iter().collect();
    ordered.sort_unstable_by_key(|p| {
        (
            if state.ascending {
                p.rss
            } else {
                u64::MAX - p.rss
            },
            p.id.pid,
        )
    });
    model.set_vec(
        ordered
            .iter()
            .map(|p| {
                let risk = snapshot.risk(p);
                ProcessRow {
                    pid: p.id.pid,
                    identity: p.id.start.to_string().into(),
                    name: p.name.clone().into(),
                    memory: bytes(p.rss).into(),
                    risk: risk.label().into(),
                    level: match risk {
                        Risk::Lower => 0,
                        Risk::Higher(_) => 1,
                        Risk::Protected(_) => 2,
                    },
                }
            })
            .collect::<Vec<_>>(),
    );
    view.set_count(ordered.len() as i32);
    publish_selection(ui, state, snapshot);
}

fn publish_selection(ui: &AppWindow, state: &State, snapshot: &Snapshot) {
    let view = ui.global::<MemoryProcesses>();
    let selected = snapshot
        .processes
        .iter()
        .find(|p| Some(p.id) == state.selected);
    view.set_selected_pid(selected.map_or(0, |p| p.id.pid));
    view.set_can_end(selected.is_some_and(|p| !matches!(snapshot.risk(p), Risk::Protected(_))));
    view.set_detail(selected.map_or_else(
        || "Select a process to see why it is marked risky or protected.".into(),
        |p| {
            format!(
                "{} · PID {} · {}",
                p.name,
                p.id.pid,
                snapshot.risk(p).reason()
            )
            .into()
        },
    ));
}

pub(super) fn install(ui: &AppWindow) -> Timer {
    let (send, requests) = mpsc::channel();
    let (replies, receive) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(request) = requests.recv() {
            let message = match request {
                Request::Scan => None,
                Request::End(id, force) => Some(match terminate(id, force) {
                    Ok(()) if force => format!("Force stop sent to PID {}.", id.pid),
                    Ok(()) => format!("Asked PID {} to stop.", id.pid),
                    Err(error) => format!("Could not end PID {}. {error}", id.pid),
                }),
            };
            if replies
                .send(Update {
                    snapshot: Snapshot::read(),
                    message,
                })
                .is_err()
            {
                break;
            }
        }
    });
    let view = ui.global::<MemoryProcesses>();
    let model = Rc::new(VecModel::default());
    view.set_rows(ModelRc::from(model.clone()));
    let state = Rc::new(RefCell::new(State::default()));
    {
        let weak = ui.as_weak();
        view.on_show(move || {
            if let Some(ui) = weak.upgrade() {
                let view = ui.global::<MemoryProcesses>();
                view.set_open(true);
                view.set_message("Reading processes…".into());
            }
        });
    }
    {
        let (weak, state, model) = (ui.as_weak(), state.clone(), model.clone());
        view.on_sort(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut state = state.borrow_mut();
            state.ascending = !state.ascending;
            ui.global::<MemoryProcesses>()
                .set_ascending(state.ascending);
            publish(&ui, &state, &model);
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        view.on_pick(move |pid, start| {
            let (Some(ui), Ok(start)) = (weak.upgrade(), start.parse()) else {
                return;
            };
            let mut state = state.borrow_mut();
            state.selected = Some(Identity { pid, start });
            if let Some(snapshot) = &state.snapshot {
                publish_selection(&ui, &state, snapshot);
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        view.on_review(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut state = state.borrow_mut();
            let Some(id) = state.selected else { return };
            let Some(snapshot) = &state.snapshot else {
                return;
            };
            let Some(process) = snapshot.processes.iter().find(|p| p.id == id) else {
                return;
            };
            let risk = snapshot.risk(process);
            if matches!(risk, Risk::Protected(_)) {
                return;
            }
            let text = format!("End {} (PID {})? {}", process.name, id.pid, risk.reason());
            state.confirmed = Some(id);
            ui.global::<MemoryProcesses>().set_confirmation(text.into());
        });
    }
    {
        let (weak, state, send) = (ui.as_weak(), state.clone(), send.clone());
        view.on_end(move |force| {
            let Some(ui) = weak.upgrade() else { return };
            let view = ui.global::<MemoryProcesses>();
            if !view.get_open() || view.get_confirmation().is_empty() || view.get_ending() {
                return;
            }
            let Some(id) = state.borrow_mut().confirmed.take() else {
                return;
            };
            view.set_confirmation("".into());
            view.set_message("Sending stop request…".into());
            view.set_ending(true);
            if send.send(Request::End(id, force)).is_err() {
                view.set_ending(false);
                view.set_message("Process worker unavailable.".into());
            }
        });
    }
    let timer = Timer::default();
    let weak = ui.as_weak();
    let mut next = Instant::now();
    timer.start(TimerMode::Repeated, Duration::from_millis(150), move || {
        let Some(ui) = weak.upgrade() else { return };
        let view = ui.global::<MemoryProcesses>();
        let mut state = state.borrow_mut();
        while let Ok(update) = receive.try_recv() {
            if let Some(message) = update.message {
                view.set_message(message.into());
                view.set_ending(false);
            } else {
                state.waiting = false;
                if view.get_message() == "Reading processes…" {
                    view.set_message("".into());
                }
            }
            match update.snapshot {
                Ok(snapshot) => state.snapshot = Some(snapshot),
                Err(error) => view.set_message(format!("Could not read processes. {error}").into()),
            }
            if view.get_open() {
                publish(&ui, &state, &model);
            }
        }
        if !view.get_open() {
            state.confirmed = None;
            next = Instant::now();
        } else if !state.waiting && !view.get_ending() && Instant::now() >= next {
            state.waiting = send.send(Request::Scan).is_ok();
            next = Instant::now() + Duration::from_secs(2);
        }
    });
    timer
}
