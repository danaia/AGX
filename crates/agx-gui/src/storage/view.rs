//! Persistent heat-map models, stable cleanup batches and explicit refresh epochs.
use super::scan;
use crate::{AppWindow, StorageCell, StorageView};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
};

#[derive(Default)]
pub(super) struct State {
    pub files: [Vec<scan::File>; 2],
    models: [Rc<VecModel<StorageCell>>; 2],
    batches: [Option<HashSet<PathBuf>>; 2],
    removed: HashMap<PathBuf, scan::File>,
    summaries: [String; 2],
    statuses: [String; 2],
    minimum_generation: u64,
    pub refreshing: bool,
}

impl State {
    pub fn bind(&self, ui: &AppWindow) {
        let view = ui.global::<StorageView>();
        view.set_sd_cells(ModelRc::from(self.models[0].clone()));
        view.set_nvme_cells(ModelRc::from(self.models[1].clone()));
    }

    pub fn publish(&self, ui: &AppWindow) {
        let view = ui.global::<StorageView>();
        view.set_sd_summary(self.summaries[0].clone().into());
        view.set_nvme_summary(self.summaries[1].clone().into());
        view.set_sd_status(self.statuses[0].clone().into());
        view.set_nvme_status(self.statuses[1].clone().into());
        view.set_refreshing(self.refreshing);
    }

    pub fn begin_refresh(&mut self, generation: u64) {
        self.minimum_generation = generation;
        self.refreshing = true;
    }

    pub fn snapshot(&mut self, generation: u64, inventories: [scan::Inventory; 2]) -> bool {
        if generation < self.minimum_generation {
            return false;
        }
        if self.refreshing {
            self.batches = Default::default();
            self.refreshing = false;
        }
        for (drive, mut inventory) in inventories.into_iter().enumerate() {
            self.summaries[drive] = summary(&inventory);
            let recreated = inventory
                .files
                .iter()
                .filter(|file| {
                    self.removed
                        .get(&file.path)
                        .is_some_and(|removed| !file.same_version(removed))
                })
                .count();
            inventory
                .files
                .retain(|file| !self.removed.contains_key(&file.path));
            if let Some(batch) = &self.batches[drive] {
                inventory.files.retain(|file| batch.contains(&file.path));
            }
            self.statuses[drive] = if inventory.mounts.is_empty() {
                status(&inventory)
            } else {
                format!(
                    "{} shown · {} eligible · {} skipped{}{}{}",
                    inventory.files.len(),
                    inventory.count,
                    inventory.errors,
                    if inventory.incomplete {
                        " · partial scan"
                    } else {
                        ""
                    },
                    if recreated > 0 {
                        format!(" · {recreated} recreated by an app (kept hidden)")
                    } else {
                        String::new()
                    },
                    if inventory.count > scan::CELL_LIMIT {
                        format!(" · largest {} files", scan::CELL_LIMIT)
                    } else {
                        String::new()
                    }
                )
            };
            self.sync(drive, inventory.files);
        }
        true
    }

    pub fn remove(&mut self, file: &scan::File) {
        self.remove_all(std::slice::from_ref(file));
    }

    pub fn remove_all(&mut self, files: &[scan::File]) {
        let paths: HashSet<_> = files.iter().map(|file| &file.path).collect();
        self.removed
            .extend(files.iter().map(|file| (file.path.clone(), file.clone())));
        for drive in 0..2 {
            if self.files[drive]
                .iter()
                .any(|candidate| paths.contains(&candidate.path))
            {
                let batch = self.batches[drive].get_or_insert_with(|| {
                    self.files[drive].iter().map(|f| f.path.clone()).collect()
                });
                batch.retain(|path| !paths.contains(path));
                let remaining = self.files[drive]
                    .iter()
                    .filter(|f| !paths.contains(&f.path))
                    .cloned()
                    .collect();
                self.sync(drive, remaining);
                self.statuses[drive] =
                    format!("{} shown · updating drive totals…", self.files[drive].len());
            }
        }
    }

    fn sync(&mut self, drive: usize, incoming: Vec<scan::File>) {
        let current = &mut self.files[drive];
        let model = &self.models[drive];
        // Remove individual rows so Slint retains the remaining tile items and
        // animates their new indices instead of rebuilding the entire map.
        let incoming_paths: HashSet<_> = incoming.iter().map(|file| file.path.clone()).collect();
        for index in (0..current.len()).rev() {
            if !incoming_paths.contains(&current[index].path) {
                current.remove(index);
                model.remove(index);
            }
        }
        for (index, file) in incoming.into_iter().enumerate() {
            if current.get(index).is_some_and(|old| old.path == file.path) {
                if current[index] != file {
                    model.set_row_data(index, cell(&file));
                    current[index] = file;
                }
            } else {
                if let Some(old) = current.iter().position(|old| old.path == file.path) {
                    current.remove(old);
                    model.remove(old);
                }
                model.insert(index, cell(&file));
                current.insert(index, file);
            }
        }
    }
}

fn cell(file: &scan::File) -> StorageCell {
    StorageCell {
        tint: size_color(file.bytes),
        path: file.path.to_string_lossy().into_owned().into(),
        detail: format!(
            "{} file size · {} on disk",
            crate::system_ui::bytes(file.bytes),
            crate::system_ui::bytes(file.allocated)
        )
        .into(),
    }
}

fn summary(inventory: &scan::Inventory) -> String {
    if inventory.mounts.is_empty() {
        return if inventory.errors == 0 {
            "Not mounted"
        } else {
            "Mount information unavailable"
        }
        .into();
    }
    format!(
        "{} / {} used",
        crate::system_ui::bytes(inventory.used),
        crate::system_ui::bytes(inventory.total)
    )
}

pub(super) fn status(inventory: &scan::Inventory) -> String {
    if inventory.mounts.is_empty() {
        return "Mounted filesystems only".into();
    }
    let coverage = if inventory.incomplete {
        "Partial scan"
    } else {
        "Personal files"
    };
    format!(
        "{coverage} · {} files · {} skipped",
        inventory.count, inventory.errors
    )
}

#[cfg(test)]
pub(super) fn cells(files: &[scan::File]) -> Vec<StorageCell> {
    files.iter().map(cell).collect()
}

pub(super) fn size_color(bytes: u64) -> slint::Color {
    let mib = bytes as f64 / 1_048_576.;
    let (from, to, heat) = if mib <= 10. {
        ([54., 112., 74.], [54., 112., 74.], 0.)
    } else if mib < 100. {
        ([54., 112., 74.], [168., 128., 54.], (mib / 10.).log10())
    } else {
        (
            [168., 128., 54.],
            [184., 65., 56.],
            (mib / 100.).log10().clamp(0., 1.),
        )
    };
    let channel = |i: usize| (from[i] + (to[i] - from[i]) * heat) as u8;
    slint::Color::from_rgb_u8(channel(0), channel(1), channel(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(index: usize) -> scan::File {
        scan::File {
            bytes: (1000 - index) as u64 * 1_048_576,
            path: format!("/home/test/file-{index}").into(),
            allocated: 4096,
            device: 1,
            inode: index as u64,
            modified: (1, 0),
        }
    }
    fn snapshot(files: Vec<scan::File>, count: usize) -> [scan::Inventory; 2] {
        [
            scan::Inventory {
                files,
                count,
                mounts: vec!["/".into()],
                ..Default::default()
            },
            scan::Inventory::default(),
        ]
    }

    #[test]
    fn deleting_does_not_backfill_tiles_or_restore_recreated_paths() {
        let mut state = State::default();
        state.snapshot(0, snapshot((0..96).map(file).collect(), 120));
        let model = state.models[0].clone();
        let deleted = state.files[0][0].clone();
        state.remove(&deleted);
        assert_eq!(model.row_count(), 95);
        assert!(model.row_data(0).unwrap().path.ends_with("file-1"));
        // A normal rescan now includes the 97th file; it must not fill the hole.
        state.snapshot(0, snapshot((1..97).map(file).collect(), 119));
        assert_eq!(model.row_count(), 95);
        assert!(state.files[0].iter().all(|f| f.path != file(96).path));
        // An app creates a different file at the deleted path.
        let mut recreated = deleted.clone();
        recreated.inode += 5000;
        recreated.modified = (2, 0);
        let mut files = vec![recreated];
        files.extend((1..96).map(file));
        state.snapshot(0, snapshot(files, 120));
        assert_eq!(model.row_count(), 95);
        assert!(state.files[0].iter().all(|f| f.path != deleted.path));
        assert!(state.statuses[0].contains("recreated by an app"));
    }

    #[test]
    fn explicit_refresh_accepts_a_new_batch_but_ignores_older_in_flight_scans() {
        let mut state = State::default();
        state.snapshot(0, snapshot((0..96).map(file).collect(), 120));
        state.remove(&file(0));
        state.begin_refresh(1);
        assert!(!state.snapshot(0, snapshot((0..96).map(file).collect(), 120)));
        assert_eq!(state.models[0].row_count(), 95);
        assert!(state.refreshing);
        assert!(state.snapshot(1, snapshot((1..97).map(file).collect(), 119)));
        assert_eq!(state.models[0].row_count(), 96);
        assert!(!state.refreshing);
    }

    #[test]
    fn removing_a_middle_tile_updates_the_existing_model_and_correct_neighbor_paths() {
        let mut state = State::default();
        state.snapshot(0, snapshot((0..4).map(file).collect(), 4));
        let model = state.models[0].clone();
        state.remove(&file(1));
        assert_eq!(model.row_count(), 3);
        assert!(model.row_data(0).unwrap().path.ends_with("file-0"));
        assert!(model.row_data(1).unwrap().path.ends_with("file-2"));
        assert!(model.row_data(2).unwrap().path.ends_with("file-3"));
        state.snapshot(0, snapshot((0..4).map(file).collect(), 4));
        assert_eq!(
            model.row_count(),
            3,
            "stale scans cannot restore a removed path"
        );
    }
    fn named_file(name: &str, inode: u64) -> scan::File {
        scan::File {
            path: format!("/home/test/{name}").into(),
            device: 1,
            inode,
            bytes: 2_097_152,
            allocated: 4096,
            modified: (1, 0),
        }
    }
    fn inventory(files: Vec<scan::File>) -> [scan::Inventory; 2] {
        [
            scan::Inventory {
                count: files.len(),
                files,
                mounts: vec!["/".into()],
                ..Default::default()
            },
            scan::Inventory::default(),
        ]
    }

    #[test]
    fn repeated_manual_refreshes_keep_deleted_paths_hidden_but_load_new_files() {
        let removed = named_file("removed", 1);
        let keep = named_file("keep", 2);
        let new = named_file("new", 3);
        let mut state = State::default();
        state.snapshot(0, inventory(vec![removed.clone(), keep.clone()]));
        state.remove(&removed);
        for generation in 1..=3 {
            let mut recreated = named_file("removed", 100 + generation);
            recreated.bytes += generation * 1_048_576;
            recreated.modified = (generation as i64 + 10, 123);
            state.begin_refresh(generation);
            assert!(!state.snapshot(
                generation - 1,
                inventory(vec![removed.clone(), keep.clone()])
            ));
            assert!(state.snapshot(
                generation,
                inventory(vec![recreated, keep.clone(), new.clone()])
            ));
            assert_eq!(state.models[0].row_count(), 2);
            assert!(state.files[0].iter().all(|f| f.path != removed.path));
            assert!(state.files[0].iter().any(|f| f.path == new.path));
            assert!(state.statuses[0].contains("recreated by an app (kept hidden)"));
        }
    }

    #[test]
    fn nvme_refresh_preserves_the_decrease_when_all_eligible_files_are_shown() {
        let mut files: Vec<_> = (0..120)
            .map(|i| {
                let mut file = file(i);
                file.path = format!("/var/lib/agx-data/users/test/file-{i}").into();
                file.device = 2;
                file
            })
            .collect();
        let removed = files[0].clone();
        let mut state = State::default();
        state.snapshot(
            0,
            [
                scan::Inventory::default(),
                scan::Inventory {
                    count: 120,
                    files: files.clone(),
                    mounts: vec!["/var/lib/agx-data".into()],
                    ..Default::default()
                },
            ],
        );
        assert_eq!(state.models[1].row_count(), 120);
        state.remove(&removed);
        assert_eq!(state.models[1].row_count(), 119);
        files.remove(0);
        for generation in 1..=3 {
            state.begin_refresh(generation);
            state.snapshot(
                generation,
                [
                    scan::Inventory::default(),
                    scan::Inventory {
                        count: 119,
                        files: files.clone(),
                        mounts: vec!["/var/lib/agx-data".into()],
                        ..Default::default()
                    },
                ],
            );
            assert_eq!(state.models[1].row_count(), 119);
            assert!(state.files[1].iter().all(|f| f.path != removed.path));
            assert_eq!(state.models[0].row_count(), 0);
        }
    }
}
