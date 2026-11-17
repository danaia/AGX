//! Bounded inventory of personal cleanup files on mounted SD and NVMe filesystems.
use super::policy::Policy;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
    ffi::CString,
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

// Show normal personal-file inventories in full. A short 96-file shortlist
// silently backfilled another file on refresh and looked like deletion failed.
pub(super) const CELL_LIMIT: usize = 4096;
const ENTRY_LIMIT: usize = 500_000;
const TIME_LIMIT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct File {
    pub bytes: u64,
    pub path: PathBuf,
    pub allocated: u64,
    pub device: u64,
    pub inode: u64,
    pub modified: (i64, i64),
}

impl File {
    pub fn same_version(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.path == other.path
            && self.bytes == other.bytes
            && self.modified == other.modified
    }
}

#[derive(Default)]
pub(super) struct Inventory {
    pub files: Vec<File>,
    pub total: u64,
    pub used: u64,
    pub count: usize,
    pub errors: usize,
    pub incomplete: bool,
    pub mounts: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Mount {
    pub path: PathBuf,
    pub nvme: bool,
}

// mountinfo escapes space, tab, newline and backslash with octal sequences.
fn unescape(value: &str) -> PathBuf {
    let mut result = Vec::new();
    let mut bytes = value.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            let digits: Vec<_> = bytes.by_ref().take(3).collect();
            if digits.len() == 3 && digits.iter().all(|b| (b'0'..=b'7').contains(b)) {
                result.push((digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + digits[2] - b'0');
            }
        } else {
            result.push(byte);
        }
    }
    PathBuf::from(std::ffi::OsStr::from_bytes(&result))
}

pub(super) fn mounts(text: &str, device_name: impl Fn(&str) -> String) -> Vec<Mount> {
    let mut found = Vec::new();
    let mut devices = HashSet::new();
    for line in text.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let fields: Vec<_> = left.split_whitespace().collect();
        let source: Vec<_> = right.split_whitespace().collect();
        if fields.len() < 6 || source.len() < 2 || fields[3] != "/" {
            continue;
        }
        let name = device_name(fields[2]);
        let nvme = name.starts_with("nvme") || source[1].starts_with("/dev/nvme");
        let sd = name.starts_with("mmcblk") || source[1].starts_with("/dev/mmcblk");
        if (nvme || sd) && devices.insert(fields[2].to_owned()) {
            found.push(Mount {
                path: unescape(fields[4]),
                nvme,
            });
        }
    }
    found
}

pub(super) fn collect(policy: &Policy) -> [Inventory; 2] {
    let Ok(info) = fs::read_to_string("/proc/self/mountinfo") else {
        return std::array::from_fn(|_| Inventory {
            errors: 1,
            ..Inventory::default()
        });
    };
    let all = mounts(&info, |device| {
        fs::read_link(format!("/sys/dev/block/{device}"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default()
    });
    std::array::from_fn(|index| {
        let paths: Vec<_> = all
            .iter()
            .filter(|m| usize::from(m.nvme) == index)
            .map(|m| m.path.clone())
            .collect();
        inventory(&paths, ENTRY_LIMIT, TIME_LIMIT, policy)
    })
}

fn capacity(path: &Path) -> Option<(u64, u64)> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // The C string lives through the call; value is read only after success.
    if unsafe { libc::statvfs(path.as_ptr(), value.as_mut_ptr()) } != 0 {
        return None;
    }
    let value = unsafe { value.assume_init() };
    Some((
        value.f_blocks.saturating_mul(value.f_frsize),
        value
            .f_blocks
            .saturating_sub(value.f_bfree)
            .saturating_mul(value.f_frsize),
    ))
}

pub(super) fn inventory(
    paths: &[PathBuf],
    limit: usize,
    time: Duration,
    policy: &Policy,
) -> Inventory {
    let mut result = Inventory {
        mounts: paths.to_vec(),
        ..Inventory::default()
    };
    let mut largest = BinaryHeap::<Reverse<File>>::new();
    let mut seen = HashSet::new();
    let start = Instant::now();
    let mut entries = 0;
    for root in paths {
        if let Some((total, used)) = capacity(root) {
            result.total = result.total.saturating_add(total);
            result.used = result.used.saturating_add(used);
        }
        let Ok(meta) = fs::symlink_metadata(root) else {
            result.errors += 1;
            continue;
        };
        let device = meta.dev();
        // Start in personal data roots, not the whole operating system tree.
        let mut pending: Vec<_> = policy
            .roots
            .iter()
            .filter(|path| path.starts_with(root) && path.exists())
            .cloned()
            .collect();
        while let Some(path) = pending.pop() {
            if entries >= limit || start.elapsed() >= time {
                result.incomplete = true;
                break;
            }
            entries += 1;
            let Ok(meta) = fs::symlink_metadata(&path) else {
                result.errors += 1;
                continue;
            };
            // Never follow symlinks, cross into other devices, or count hard links twice.
            if meta.dev() != device
                || meta.file_type().is_symlink()
                || !seen.insert((meta.dev(), meta.ino()))
            {
                continue;
            }
            if !policy.contains(&path) {
                continue;
            }
            if meta.is_dir() {
                match fs::read_dir(&path) {
                    Ok(children) => {
                        for child in children {
                            if pending.len() >= limit || start.elapsed() >= time {
                                result.incomplete = true;
                                break;
                            }
                            match child {
                                Ok(child) => pending.push(child.path()),
                                Err(_) => result.errors += 1,
                            }
                        }
                    }
                    Err(_) => result.errors += 1,
                }
            } else if policy.eligible(&path, &meta) {
                result.count += 1;
                largest.push(Reverse(File {
                    bytes: meta.len(),
                    path,
                    allocated: meta.blocks().saturating_mul(512),
                    device: meta.dev(),
                    inode: meta.ino(),
                    modified: (meta.mtime(), meta.mtime_nsec()),
                }));
                if largest.len() > CELL_LIMIT {
                    largest.pop();
                }
            }
        }
    }
    result.files = largest.into_iter().map(|f| f.0).collect();
    result.files.sort_by(|a, b| b.cmp(a));
    result
}
