//! Discover the signed-in account's recoverable Trash items across mounted volumes.
use super::{Stored, nvme_trash, private_dir, volume_trash};
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};

#[derive(Clone)]
pub struct Entry {
    pub stored: Stored,
    pub bytes: Option<u64>,
    identity: (u64, u64, i64, i64),
}

fn identity(meta: &fs::Metadata) -> (u64, u64, i64, i64) {
    (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
}

impl Entry {
    pub fn apply(&self, permanent: bool) -> io::Result<()> {
        let file = &self.stored.file;
        let meta = fs::symlink_metadata(file)?;
        if identity(&meta) != self.identity {
            return Err(io::Error::other(
                "This Trash item changed. Refresh and select it again.",
            ));
        }
        private_dir(file.parent().unwrap())?;
        private_dir(self.stored.info.parent().unwrap())?;
        if permanent {
            if meta.is_dir() {
                fs::remove_dir_all(file)?;
            } else {
                fs::remove_file(file)?;
            }
            let _ = fs::remove_file(&self.stored.info);
            Ok(())
        } else {
            super::restore(&self.stored)
        }
    }
}

pub fn list(home: Option<PathBuf>) -> io::Result<Vec<Entry>> {
    let mounts = fs::read_to_string("/proc/self/mounts")?;
    let mut roots: Vec<_> = super::super::places::drives(&mounts)
        .into_iter()
        .map(|top| (volume_trash(&top), Some(top)))
        .collect();
    if let Some(dir) = nvme_trash() {
        roots.push((dir, Some("/var/lib/agx-data".into())));
    }
    if let Some(home) = home {
        roots.push((home, None));
    }
    let mut entries = Vec::new();
    let mut first_error = None;
    for (root, top) in roots {
        if let Err(error) = read_root(&root, top.as_deref(), &mut entries)
            && error.kind() != io::ErrorKind::NotFound
        {
            first_error.get_or_insert(error);
        }
    }
    if entries.is_empty()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    entries.sort_by(|a, b| {
        b.identity
            .2
            .cmp(&a.identity.2)
            .then_with(|| a.stored.file.cmp(&b.stored.file))
    });
    entries.dedup_by(|a, b| a.stored.file == b.stored.file);
    Ok(entries)
}

fn read_root(root: &Path, top: Option<&Path>, entries: &mut Vec<Entry>) -> io::Result<()> {
    private_dir(root)?;
    let files = root.join("files");
    let info = root.join("info");
    private_dir(&files)?;
    private_dir(&info)?;
    for item in fs::read_dir(&info)?.take(4096).flatten() {
        let path = item.path();
        if path.extension() != Some(OsStr::new("trashinfo")) {
            continue;
        }
        let Some(stem) = path.file_stem().filter(|stem| {
            matches!(
                Path::new(stem).components().next(),
                Some(Component::Normal(_))
            )
        }) else {
            continue;
        };
        let file = files.join(stem);
        let Ok(meta) = fs::symlink_metadata(&file) else {
            continue;
        };
        let Some(original) = original_path(&path, top) else {
            continue;
        };
        if original.starts_with(root) {
            continue;
        }
        entries.push(Entry {
            stored: Stored {
                original,
                file,
                info: path,
            },
            bytes: meta.is_file().then_some(meta.len()),
            identity: identity(&meta),
        });
    }
    Ok(())
}

fn original_path(info: &Path, top: Option<&Path>) -> Option<PathBuf> {
    if !fs::symlink_metadata(info).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(info)
        .ok()?
        .take(32_768)
        .read_to_string(&mut text)
        .ok()?;
    let mut input = text
        .lines()
        .find_map(|line| line.strip_prefix("Path="))?
        .as_bytes()
        .iter()
        .copied();
    let mut decoded = Vec::new();
    while let Some(byte) = input.next() {
        decoded.push(if byte == b'%' {
            let hex = |b: u8| (b as char).to_digit(16).map(|n| n as u8);
            hex(input.next()?)? * 16 + hex(input.next()?)?
        } else {
            byte
        });
    }
    let path = Path::new(OsStr::from_bytes(&decoded));
    if path.file_name().is_none() || path.components().any(|p| matches!(p, Component::ParentDir)) {
        return None;
    }
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(top?.join(path))
    }
}
