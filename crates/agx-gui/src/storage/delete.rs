//! Confirmed deletion of one inventory file, with pinned parents and identity checks.
use super::{policy::Policy, scan::File};
use crate::files::trash;
use std::{
    ffi::CString,
    fs, io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
pub(super) fn name(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| invalid("Invalid file path"))
}

// Walk each parent through a directory descriptor. No ancestor symlink can
// redirect cleanup into a system or other account's directory.
pub(super) fn parent(path: &Path) -> io::Result<fs::File> {
    if !path.is_absolute() {
        return Err(invalid("An absolute file path is required"));
    }
    let mut directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for component in path
        .parent()
        .ok_or_else(|| invalid("Missing parent"))?
        .components()
    {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => {
                let part =
                    CString::new(part.as_bytes()).map_err(|_| invalid("Invalid directory"))?;
                let fd = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        part.as_ptr(),
                        libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                directory = unsafe { fs::File::from_raw_fd(fd) };
            }
            _ => return Err(invalid("Invalid parent directory")),
        }
    }
    Ok(directory)
}

fn matches(file: &File, meta: &fs::Metadata) -> bool {
    meta.is_file()
        && meta.dev() == file.device
        && meta.ino() == file.inode
        && meta.len() == file.bytes
        && (meta.mtime(), meta.mtime_nsec()) == file.modified
}

fn metadata(directory: &fs::File, entry: &CString) -> io::Result<fs::Metadata> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            entry.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe { fs::File::from_raw_fd(fd) }.metadata()
}

fn rename(directory: &fs::File, from: &CString, to: &CString) -> io::Result<()> {
    // RENAME_NOREPLACE never overwrites another file, including during rollback.
    if unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn remove(file: &File, policy: &Policy, home: Option<PathBuf>) -> io::Result<()> {
    if !policy.contains(&file.path) {
        return Err(invalid("This is not a personal cleanup file"));
    }
    trash_checked(file, home, |meta, moved| {
        moved || policy.eligible(&file.path, meta)
    })
}

pub(super) fn trash_checked(
    file: &File,
    home: Option<PathBuf>,
    allowed: impl Fn(&fs::Metadata, bool) -> bool,
) -> io::Result<()> {
    act_checked(file, allowed, |directory, temporary| {
        trash::store(&file.path, home, |destination| {
            let temporary = Path::new(std::ffi::OsStr::from_bytes(temporary.as_bytes()));
            trash::rename_noreplace(directory.as_raw_fd(), temporary, destination)
        })
        .map(drop)
    })
}

pub(super) fn remove_checked(
    file: &File,
    allowed: impl Fn(&fs::Metadata, bool) -> bool,
) -> io::Result<()> {
    act_checked(file, allowed, |directory, temporary| {
        if unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })
}

fn act_checked(
    file: &File,
    allowed: impl Fn(&fs::Metadata, bool) -> bool,
    commit: impl FnOnce(&fs::File, &CString) -> io::Result<()>,
) -> io::Result<()> {
    let directory = parent(&file.path)?;
    let entry = name(Path::new(
        file.path
            .file_name()
            .ok_or_else(|| invalid("Missing file name"))?,
    ))?;
    let meta = metadata(&directory, &entry)?;
    if !matches(file, &meta) || !allowed(&meta, false) {
        return Err(invalid(
            "The file changed or is no longer removable. Close this dialog and select it again.",
        ));
    }
    let temporary = CString::new(format!(
        ".angi-delete-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
    .unwrap();
    rename(&directory, &entry, &temporary)?;
    // Verify after moving as well: a replacement between check and rename must
    // be restored, never deleted. Only the exact confirmed file is unlinked.
    let result = (|| {
        let meta = metadata(&directory, &temporary)?;
        if !matches(file, &meta) || !allowed(&meta, true) {
            return Err(invalid(
                "The file changed before deletion. Please select it again.",
            ));
        }
        commit(&directory, &temporary)
    })();
    if let Err(error) = result {
        if let Err(restore) = rename(&directory, &temporary, &entry) {
            return Err(invalid(&format!(
                "{error}. File preserved as {} beside the original: {restore}",
                temporary.to_string_lossy()
            )));
        }
        return Err(error);
    }
    Ok(())
}
