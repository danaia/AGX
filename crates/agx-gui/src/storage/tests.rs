use super::{
    delete,
    policy::Policy,
    scan,
    view::{cells, size_color, status},
};
use std::{fs, os::unix::fs::symlink, path::PathBuf, time::Duration};

pub(super) struct Fixture(pub PathBuf);
impl Fixture {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("agx-storage-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn policy(&self) -> Policy {
        Policy {
            uid: unsafe { libc::geteuid() },
            roots: vec![self.0.clone()],
        }
    }
    fn file(&self, name: &str, bytes: u64) {
        fs::File::create(self.0.join(name))
            .unwrap()
            .set_len(bytes)
            .unwrap();
    }
    fn scan(&self) -> scan::Inventory {
        scan::inventory(
            &[self.0.clone()],
            10_000,
            Duration::from_secs(2),
            &self.policy(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn discovers_sd_root_alias_and_nvme_without_double_counting_bind_mounts() {
    let info = "20 1 179:2 / / rw - ext4 /dev/root rw\n\
        21 1 259:1 / /var/lib/agx\\040data rw - ext4 /dev/nvme0n1p1 rw\n\
        22 1 259:1 / /duplicate rw - ext4 /dev/nvme0n1p1 rw\n\
        23 1 0:1 / /proc rw - proc proc rw\n\
        24 1 179:2 /usr /bind rw - ext4 /dev/root rw";
    let mounts = scan::mounts(info, |id| {
        if id == "179:2" { "mmcblk0p2" } else { "" }.into()
    });
    assert_eq!(
        mounts,
        vec![
            scan::Mount {
                path: "/".into(),
                nvme: false
            },
            scan::Mount {
                path: "/var/lib/agx data".into(),
                nvme: true
            },
        ]
    );
}

#[test]
fn inventory_does_not_follow_symlinks_or_count_hard_links_twice() {
    let fixture = Fixture::new("links");
    fixture.file("small", 1_048_576);
    fixture.file("large", 2_097_152);
    fs::hard_link(fixture.0.join("large"), fixture.0.join("hard-link")).unwrap();
    symlink(&fixture.0, fixture.0.join("loop")).unwrap();
    symlink("/proc", fixture.0.join("foreign-filesystem")).unwrap();
    let inventory = fixture.scan();
    assert_eq!(inventory.count, 2);
    assert_eq!(
        inventory.files.iter().map(|f| f.bytes).collect::<Vec<_>>(),
        [2_097_152, 1_048_576]
    );
    assert!(!inventory.incomplete);
    assert_eq!(inventory.errors, 0);
}

#[test]
fn inventories_small_downloads_on_their_actual_volume() {
    let fixture = Fixture::new("small-download");
    let path = "agx-web-users/1000/downloads";
    fs::create_dir_all(fixture.0.join(path)).unwrap();
    fixture.file(&format!("{path}/image.jpe"), 16_264);
    fixture.file("small-outside.jpe", 16_264);
    symlink(fixture.0.join(path), fixture.0.join("Downloads")).unwrap();

    let inventory = fixture.scan();
    assert_eq!(inventory.files.len(), 1);
    let file = &inventory.files[0];
    assert_eq!(file.path, fixture.0.join(path).join("image.jpe"));
    assert!(super::category::Category::Downloads.includes(&file.path));
    delete::remove(file, &fixture.policy(), Some(fixture.0.join(".Trash"))).unwrap();
    assert!(!file.path.exists());
    assert!(fixture.0.join("small-outside.jpe").exists());
}

#[test]
fn keeps_the_largest_files_and_reports_sparse_allocation_separately() {
    let fixture = Fixture::new("largest");
    for i in 1..=scan::CELL_LIMIT + 24 {
        fixture.file(&format!("file-{i}"), 1_048_576 + i as u64);
    }
    let sparse = fs::File::create(fixture.0.join("sparse")).unwrap();
    sparse.set_len(67_108_864).unwrap();
    let inventory = fixture.scan();
    assert_eq!(inventory.count, scan::CELL_LIMIT + 25);
    assert_eq!(inventory.files.len(), scan::CELL_LIMIT);
    assert_eq!(inventory.files[0].bytes, 67_108_864);
    assert!(inventory.files[0].allocated < inventory.files[0].bytes);
    assert!(inventory.files.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    let model = cells(&inventory.files);
    assert!(model[0].detail.contains("64.0 MiB file size"));
    assert!(model[0].detail.contains("on disk"));
    assert!(model[0].tint.red() > model.last().unwrap().tint.red());
}

#[test]
fn bounded_and_unreadable_scans_are_labeled_without_inventing_file_sizes() {
    let fixture = Fixture::new("partial");
    fs::write(fixture.0.join("file"), b"data").unwrap();
    let partial = scan::inventory(
        &[fixture.0.clone()],
        1,
        Duration::from_secs(2),
        &fixture.policy(),
    );
    assert!(partial.incomplete);
    assert!(status(&partial).starts_with("Partial scan"));
    let missing = scan::inventory(
        &[fixture.0.join("missing")],
        10,
        Duration::from_secs(2),
        &fixture.policy(),
    );
    assert_eq!(missing.errors, 1);
    assert!(missing.files.is_empty());
    assert!(status(&missing).starts_with("Personal files"));
}

#[test]
fn only_owned_large_personal_files_are_cleanup_candidates() {
    let fixture = Fixture::new("policy");
    fixture.file("download.zip", 2_097_152);
    fixture.file("small.txt", 100);
    fixture.file(".angi-delete-1-0", 2_097_152);
    for directory in [".git", ".config", ".ssh", ".local/bin"] {
        fs::create_dir_all(fixture.0.join(directory)).unwrap();
        fixture.file(&format!("{directory}/important"), 4_194_304);
    }
    let inventory = fixture.scan();
    assert_eq!(inventory.files.len(), 1);
    assert!(inventory.files[0].path.ends_with("download.zip"));
    let mut other_account = fixture.policy();
    other_account.uid = other_account.uid.wrapping_add(1);
    assert!(
        scan::inventory(
            &[fixture.0.clone()],
            100,
            Duration::from_secs(2),
            &other_account
        )
        .files
        .is_empty()
    );
    assert!(
        !fixture
            .policy()
            .contains(std::path::Path::new("/usr/lib/system-library.so"))
    );
}

#[test]
fn colors_use_absolute_size_thresholds() {
    let green = size_color(5 * 1_048_576);
    let amber = size_color(100 * 1_048_576);
    let red = size_color(1024 * 1_048_576);
    assert!(green.green() > green.red());
    assert!(amber.red() > green.red());
    assert!(red.red() > red.green() * 2);
    assert_eq!(red, size_color(10 * 1024 * 1_048_576));
}

#[test]
fn deletion_removes_only_the_selected_personal_file_and_rescan_excludes_it() {
    let fixture = Fixture::new("delete");
    fixture.file("chosen", 2_097_152);
    fixture.file("keep", 1_048_576);
    let file = fixture.scan().files[0].clone();
    delete::remove(&file, &fixture.policy(), Some(fixture.0.join(".Trash"))).unwrap();
    assert!(!fixture.0.join("chosen").exists());
    assert!(fixture.0.join("keep").exists());
    assert_eq!(fixture.scan().files.len(), 1);
    let entries = crate::files::trash::list(Some(fixture.0.join(".Trash"))).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.stored.original == file.path)
        .unwrap();
    assert!(entry.stored.file.exists());
    fixture.file("chosen", 1);
    assert!(entry.apply(false).is_err(), "restore must not overwrite");
    fs::remove_file(&file.path).unwrap();
    entry.apply(false).unwrap();
    assert_eq!(fs::metadata(&file.path).unwrap().len(), file.bytes);
    delete::remove(&file, &fixture.policy(), Some(fixture.0.join(".Trash"))).unwrap();
    let entries = crate::files::trash::list(Some(fixture.0.join(".Trash"))).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.stored.original == file.path)
        .unwrap();
    entry.apply(true).unwrap();
    assert!(!entry.stored.file.exists());
    assert!(!entry.stored.info.exists());
}

#[test]
fn deletion_refuses_replaced_modified_and_out_of_scope_files() {
    let fixture = Fixture::new("changed");
    fixture.file("chosen", 2_097_152);
    let file = fixture.scan().files[0].clone();
    fs::rename(&file.path, fixture.0.join("original")).unwrap();
    fixture.file("chosen", file.bytes);
    assert!(delete::remove(&file, &fixture.policy(), Some(fixture.0.join(".Trash"))).is_err());
    assert!(file.path.exists());
    let changed = fixture
        .scan()
        .files
        .into_iter()
        .find(|f| f.path == file.path)
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&changed.path)
        .unwrap()
        .set_len(changed.bytes + 1)
        .unwrap();
    assert!(delete::remove(&changed, &fixture.policy(), Some(fixture.0.join(".Trash"))).is_err());
    let outside = Fixture::new("outside");
    assert!(delete::remove(&changed, &outside.policy(), Some(outside.0.join(".Trash"))).is_err());
    assert!(changed.path.exists());
}

#[test]
fn deletion_refuses_a_parent_replaced_with_a_symlink() {
    let fixture = Fixture::new("parent");
    let outside = Fixture::new("parent-outside");
    fs::create_dir(fixture.0.join("downloads")).unwrap();
    fixture.file("downloads/chosen", 2_097_152);
    outside.file("chosen", 2_097_152);
    let file = fixture.scan().files[0].clone();
    fs::rename(fixture.0.join("downloads"), fixture.0.join("saved")).unwrap();
    symlink(&outside.0, fixture.0.join("downloads")).unwrap();
    assert!(delete::remove(&file, &fixture.policy(), Some(fixture.0.join(".Trash"))).is_err());
    assert!(outside.0.join("chosen").exists());
    assert!(fixture.0.join("saved/chosen").exists());
}

#[test]
fn deletion_is_published_while_an_inventory_scan_is_still_blocked() {
    use super::worker::{Update, Worker};
    use std::sync::mpsc;
    let fixture = Fixture::new("async-delete");
    fixture.file("chosen", 2_097_152);
    fixture.file("keep", 1_048_576);
    let mut stale = Some(fixture.scan());
    let file = stale.as_ref().unwrap().files[0].clone();
    let (publish, updates) = mpsc::channel();
    let (started, scan_started) = mpsc::channel();
    let (release, blocked_scan) = mpsc::channel();
    let worker = Worker::with_collector(
        fixture.policy(),
        publish,
        Some(fixture.0.join(".Trash")),
        move || {
            if let Some(inventory) = stale.take() {
                started.send(()).unwrap();
                blocked_scan.recv_timeout(Duration::from_secs(5)).unwrap();
                [inventory, scan::Inventory::default()]
            } else {
                std::array::from_fn(|_| scan::Inventory::default())
            }
        },
    );
    scan_started.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.delete(file.clone()).unwrap();
    let Update::Deleted(completed, result) = updates.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("delete completion must arrive before the blocked scan finishes")
    };
    result.unwrap();
    assert_eq!(completed, file);
    assert!(!file.path.exists());
    release.send(()).unwrap();
    let Update::Snapshot(generation, inventories) =
        updates.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("expected snapshot")
    };
    let mut state = super::view::State::default();
    state.snapshot(generation, inventories);
    state.remove(&completed);
    assert_eq!(state.files[0].len(), 1, "deleted tiles stay absent");
    assert!(state.files[0][0].path.ends_with("keep"));
    worker.stop();
}

#[test]
fn asynchronous_delete_failure_preserves_the_file() {
    use super::worker::{Update, Worker};
    use std::sync::mpsc;
    let fixture = Fixture::new("async-failure");
    fixture.file("chosen", 2_097_152);
    let file = fixture.scan().files[0].clone();
    fs::OpenOptions::new()
        .write(true)
        .open(&file.path)
        .unwrap()
        .set_len(file.bytes + 1)
        .unwrap();
    let (publish, updates) = mpsc::channel();
    let worker = Worker::with_collector(
        fixture.policy(),
        publish,
        Some(fixture.0.join(".Trash")),
        || std::array::from_fn(|_| scan::Inventory::default()),
    );
    worker.delete(file.clone()).unwrap();
    loop {
        if let Update::Deleted(completed, result) =
            updates.recv_timeout(Duration::from_secs(2)).unwrap()
        {
            assert!(result.is_err());
            assert_eq!(completed, file);
            assert!(file.path.exists());
            break;
        }
    }
    worker.stop();
}
