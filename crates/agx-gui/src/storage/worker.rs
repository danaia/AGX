//! Independent deletion and inventory workers: deletion never waits for a scan.
use super::{cache, delete, policy::Policy, scan};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

pub(super) enum Update {
    Snapshot(u64, [scan::Inventory; 2]),
    Deleted(scan::File, Result<(), String>),
    CacheChecked(Result<cache::Plan, String>),
    CacheCleared(cache::Outcome),
}

enum ScanCommand {
    Refresh,
    Stop,
}
enum DeleteCommand {
    File(scan::File),
    CheckCache,
    ClearCache(cache::Plan),
    Stop,
}

#[derive(Clone)]
pub(super) struct Worker {
    scan: mpsc::Sender<ScanCommand>,
    delete: mpsc::Sender<DeleteCommand>,
    generation: Arc<AtomicU64>,
}

impl Worker {
    pub fn start(policy: Policy, publish: mpsc::Sender<Update>) -> Self {
        let scan_policy = policy.clone();
        Self::with_collector(policy, publish, move || scan::collect(&scan_policy))
    }

    pub(super) fn with_collector(
        policy: Policy,
        publish: mpsc::Sender<Update>,
        mut collect: impl FnMut() -> [scan::Inventory; 2] + Send + 'static,
    ) -> Self {
        let (scan, requests) = mpsc::channel();
        let snapshots = publish.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let scan_generation = generation.clone();
        std::thread::spawn(move || {
            'scanning: loop {
                let generation = scan_generation.load(Ordering::Acquire);
                if snapshots
                    .send(Update::Snapshot(generation, collect()))
                    .is_err()
                {
                    break;
                }
                match requests.recv_timeout(Duration::from_secs(60)) {
                    Ok(ScanCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(ScanCommand::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                // Several deletions during one scan need only one follow-up scan.
                while let Ok(command) = requests.try_recv() {
                    if matches!(command, ScanCommand::Stop) {
                        break 'scanning;
                    }
                }
            }
        });
        let (delete, deletions) = mpsc::channel();
        let refresh = scan.clone();
        std::thread::spawn(move || {
            while let Ok(command) = deletions.recv() {
                let (update, rescan) = match command {
                    DeleteCommand::Stop => break,
                    DeleteCommand::File(file) => {
                        let result =
                            delete::remove(&file, &policy).map_err(|error| error.to_string());
                        (Update::Deleted(file, result), true)
                    }
                    DeleteCommand::CheckCache => (
                        Update::CacheChecked(
                            cache::Plan::check(policy.uid).map_err(|error| error.to_string()),
                        ),
                        false,
                    ),
                    DeleteCommand::ClearCache(plan) => (Update::CacheCleared(plan.clear()), true),
                };
                // Publish completion before requesting any expensive inventory work.
                if publish.send(update).is_err() {
                    break;
                }
                if rescan {
                    let _ = refresh.send(ScanCommand::Refresh);
                }
            }
        });
        Self {
            scan,
            delete,
            generation,
        }
    }

    pub fn delete(&self, file: scan::File) -> Result<(), &'static str> {
        self.delete
            .send(DeleteCommand::File(file))
            .map_err(|_| "Could not start deletion")
    }

    pub fn check_cache(&self) -> bool {
        self.delete.send(DeleteCommand::CheckCache).is_ok()
    }
    pub fn clear_cache(&self, plan: cache::Plan) -> bool {
        self.delete.send(DeleteCommand::ClearCache(plan)).is_ok()
    }

    pub fn refresh(&self) -> Result<u64, &'static str> {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.scan
            .send(ScanCommand::Refresh)
            .map_err(|_| "Could not refresh files")?;
        Ok(generation)
    }

    pub fn stop(&self) {
        let _ = self.delete.send(DeleteCommand::Stop);
        let _ = self.scan.send(ScanCommand::Stop);
    }
}
