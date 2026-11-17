//! Background threads. One scans (listing, search, change detection) and one runs file operations,
//! so a long copy never delays navigation and the UI thread never touches the disk.
use super::{
    entry::Entry,
    listing,
    ops::{self, Op, Outcome},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, SystemTime},
};

pub const PANES: usize = 2;
/// How often a watched folder is checked for changes made by other programs.
const WATCH_INTERVAL: Duration = Duration::from_millis(750);

pub enum Reply {
    Listed { pane: usize, request: u64, dir: PathBuf, result: Result<Vec<Entry>, String> },
    Found { pane: usize, request: u64, entries: Vec<Entry> },
    /// The watched folder was modified by someone else.
    Changed { pane: usize },
    Progress { pane: usize, done: usize, total: usize, current: String },
    Done { pane: usize, request: u64, outcome: Outcome },
}

enum Scan {
    List { pane: usize, request: u64, dir: PathBuf },
    Search { pane: usize, request: u64, dir: PathBuf, query: String },
}

pub struct Worker {
    scan: Sender<Scan>,
    ops: Sender<(usize, u64, Op)>,
    /// The newest request per pane; older scans stop early.
    latest: Arc<[AtomicU64; PANES]>,
}

impl Worker {
    pub fn start(replies: Sender<Reply>) -> Self {
        let latest = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);
        let (scan, scans) = mpsc::channel();
        let (ops, jobs) = mpsc::channel();
        let (out, newest) = (replies.clone(), latest.clone());
        thread::Builder::new().name("files-scan".into()).spawn(move || scan_loop(scans, out, newest)).ok();
        thread::Builder::new().name("files-ops".into()).spawn(move || ops_loop(jobs, replies)).ok();
        Self { scan, ops, latest }
    }

    pub fn list(&self, pane: usize, request: u64, dir: PathBuf) {
        self.latest[pane].store(request, Ordering::Relaxed);
        let _ = self.scan.send(Scan::List { pane, request, dir });
    }

    pub fn search(&self, pane: usize, request: u64, dir: PathBuf, query: String) {
        self.latest[pane].store(request, Ordering::Relaxed);
        let _ = self.scan.send(Scan::Search { pane, request, dir, query });
    }

    pub fn run(&self, pane: usize, request: u64, op: Op) {
        let _ = self.ops.send((pane, request, op));
    }
}

fn stamp(dir: &Path) -> Option<SystemTime> {
    fs::metadata(dir).and_then(|m| m.modified()).ok()
}

fn scan_loop(jobs: Receiver<Scan>, replies: Sender<Reply>, latest: Arc<[AtomicU64; PANES]>) {
    let mut watched: [Option<(PathBuf, Option<SystemTime>)>; PANES] = [None, None];
    loop {
        let first = match jobs.recv_timeout(WATCH_INTERVAL) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                for (pane, slot) in watched.iter_mut().enumerate() {
                    if let Some((dir, seen)) = slot {
                        let now = stamp(dir);
                        if now != *seen {
                            *seen = now;
                            if replies.send(Reply::Changed { pane }).is_err() {
                                return;
                            }
                        }
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // Skip scans a newer request for the same pane has already replaced.
        let mut batch = vec![first];
        batch.extend(jobs.try_iter());
        for job in batch {
            let (pane, request) = match &job {
                Scan::List { pane, request, .. } | Scan::Search { pane, request, .. } => (*pane, *request),
            };
            if latest[pane].load(Ordering::Relaxed) != request {
                continue;
            }
            let reply = match job {
                Scan::List { dir, .. } => {
                    // Stamp first: a change during the read then shows up on the next check.
                    let seen = stamp(&dir);
                    let result = listing::list(&dir);
                    watched[pane] = result.is_ok().then(|| (dir.clone(), seen));
                    Reply::Listed { pane, request, dir, result }
                }
                Scan::Search { dir, query, .. } => {
                    watched[pane] = None;
                    Reply::Found { pane, request, entries: listing::search(&dir, &query, request, &latest[pane]) }
                }
            };
            if replies.send(reply).is_err() {
                return;
            }
        }
    }
}

fn ops_loop(jobs: Receiver<(usize, u64, Op)>, replies: Sender<Reply>) {
    for (pane, request, op) in jobs {
        let progress = replies.clone();
        let outcome = ops::run(op, &mut |done, total, current| {
            let _ = progress.send(Reply::Progress { pane, done, total, current: current.to_owned() });
        });
        if replies.send(Reply::Done { pane, request, outcome }).is_err() {
            return;
        }
    }
}
