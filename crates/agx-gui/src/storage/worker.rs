//! Independent deletion and inventory workers: deletion never waits for a scan.
use super::{cache, delete, policy::Policy, scan};
use crate::files::trash;
use std::{
    path::PathBuf,
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
    TrashEmptied(Result<(), String>),
    TrashListed(Result<Vec<trash::Entry>, String>),
    TrashActed(trash::Entry, bool, Result<(), String>),
    CacheTiles(Result<Vec<scan::File>, String>),
    CacheChecked(Result<cache::Plan, String>),
    CacheCleared(cache::Outcome),
}

enum ScanCommand {
    Refresh,
    Stop,
}
enum DeleteCommand {
    EmptyTrash(Vec<trash::Entry>),
    ListTrash,
    TrashAction(trash::Entry, bool),
    File(scan::File, bool),
    ListCache,
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
        Self::with_collector(policy, publish, trash::home_trash(), move || {
            scan::collect(&scan_policy)
        })
    }

    pub(super) fn with_collector(
        policy: Policy,
        publish: mpsc::Sender<Update>,
        home: Option<PathBuf>,
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
            let mut cache_tiles: Option<cache::Plan> = None;
            while let Ok(command) = deletions.recv() {
                let (update, rescan) = match command {
                    DeleteCommand::Stop => break,
                    DeleteCommand::File(file, verified_cache) => {
                        let result = if verified_cache {
                            cache_tiles
                                .as_mut()
                                .ok_or_else(|| {
                                    std::io::Error::other("Refresh the Cache tab first.")
                                })
                                .and_then(|plan| plan.remove_tile(&file, home.clone()))
                        } else {
                            delete::remove(&file, &policy, home.clone())
                        }
                        .map_err(|error| error.to_string());
                        (Update::Deleted(file, result), true)
                    }
                    DeleteCommand::EmptyTrash(entries) => {
                        let mut errors = Vec::new();
                        for entry in entries {
                            if let Err(error) = entry.apply(true) { errors.push(error.to_string()); }
                        }
                        let result = if errors.is_empty() { Ok(()) } else {
                            Err(format!("Could not delete {} item(s): {}", errors.len(), errors[0]))
                        };
                        (Update::TrashEmptied(result), true)
                    }
                    DeleteCommand::ListTrash => (
                        Update::TrashListed(trash::list(home.clone()).map_err(|e| e.to_string())),
                        false,
                    ),
                    DeleteCommand::TrashAction(entry, permanent) => {
                        let result = entry.apply(permanent).map_err(|e| e.to_string());
                        (Update::TrashActed(entry, permanent, result), true)
                    }
                    DeleteCommand::ListCache => {
                        let result = cache::Plan::check(policy.uid)
                            .map(|plan| {
                                let files = plan.tiles();
                                cache_tiles = Some(plan);
                                files
                            })
                            .map_err(|error| {
                                cache_tiles = None;
                                error.to_string()
                            });
                        (Update::CacheTiles(result), false)
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

    #[cfg(test)]
    pub fn delete(&self, file: scan::File) -> Result<(), &'static str> {
        self.delete_tile(file, false)
    }

    pub fn delete_tile(&self, file: scan::File, verified_cache: bool) -> Result<(), &'static str> {
        self.delete
            .send(DeleteCommand::File(file, verified_cache))
            .map_err(|_| "Could not start deletion")
    }

    pub fn empty_trash(&self, entries: Vec<trash::Entry>) -> bool {
        self.delete.send(DeleteCommand::EmptyTrash(entries)).is_ok()
    }

    pub fn list_trash(&self) -> bool {
        self.delete.send(DeleteCommand::ListTrash).is_ok()
    }

    pub fn act_on_trash(&self, entry: trash::Entry, permanent: bool) -> bool {
        self.delete
            .send(DeleteCommand::TrashAction(entry, permanent))
            .is_ok()
    }

    pub fn list_cache(&self) -> bool {
        self.delete.send(DeleteCommand::ListCache).is_ok()
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
