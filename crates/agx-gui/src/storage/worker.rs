//! Independent deletion and inventory workers: deletion never waits for a scan.
use super::{delete, policy::Policy, scan};
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
}

enum ScanCommand {
    Refresh,
    Stop,
}
enum DeleteCommand {
    File(scan::File),
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
            while let Ok(DeleteCommand::File(file)) = deletions.recv() {
                let result = delete::remove(&file, &policy).map_err(|error| error.to_string());
                // Publish completion before requesting any expensive inventory work.
                if publish.send(Update::Deleted(file, result)).is_err() {
                    break;
                }
                let _ = refresh.send(ScanCommand::Refresh);
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
