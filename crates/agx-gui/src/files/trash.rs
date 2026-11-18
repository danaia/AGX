//! Moving files to the freedesktop.org Trash, so deleting is recoverable.
//!
//! Files on the home volume go to `$XDG_DATA_HOME/Trash`; files on any other volume go to
//! `<volume>/.Trash-<uid>`. Keeping each file on its own volume makes trashing a rename, never a
//! copy, which matters for large folders on the NVMe data volume.
mod inventory;
pub use inventory::{Entry, list};

use std::{
    ffi::CString,
    fs,
    io::{self, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt},
    },
    path::{Path, PathBuf},
};

/// Where a trashed file lives, enough to put it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub original: PathBuf,
    pub file: PathBuf,
    pub info: PathBuf,
}

pub fn home_trash() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))?;
    Some(data.join("Trash"))
}

fn device(path: &Path) -> io::Result<u64> {
    fs::metadata(path).map(|m| m.dev())
}

/// The trash directory for a file's volume, created if needed.
fn trash_dir(path: &Path, home: Option<PathBuf>) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Cannot trash the root folder."))?;
    let dev = device(parent)?;
    if let Some(home) = home {
        let anchor = home.ancestors().find(|a| a.exists());
        if anchor.and_then(|a| device(a).ok()) == Some(dev) {
            return Ok(home);
        }
    }
    let mut top = parent.to_path_buf();
    while let Some(up) = top.parent() {
        if device(up).ok() != Some(dev) {
            break;
        }
        top = up.to_path_buf();
    }
    let standard = volume_trash(&top);
    if let Some(private) = nvme_trash().filter(|dir| {
        top == Path::new("/var/lib/agx-data")
            && !standard.exists()
            && dir.parent().and_then(|parent| device(parent).ok()) == Some(dev)
    }) {
        return Ok(private);
    }
    Ok(standard)
}

fn uid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}
fn volume_trash(top: &Path) -> PathBuf {
    top.join(format!(".Trash-{}", uid()))
}
fn nvme_trash() -> Option<PathBuf> {
    let account = super::places::home();
    let dir = Path::new("/var/lib/agx-data/users").join(account.file_name()?);
    private_dir(&dir).ok()?;
    Some(dir.join(".Trash"))
}

fn private_dir(dir: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(dir)?;
    if meta.is_dir() && meta.uid() == uid() && meta.mode() & 0o077 == 0 {
        Ok(())
    } else {
        Err(io::Error::other(
            "Trash requires a private directory owned by your account.",
        ))
    }
}

fn ensure(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir)?;
    private_dir(dir)?;
    for sub in ["files", "info"] {
        let path = dir.join(sub);
        builder.create(&path)?;
        private_dir(&path)?;
    }
    Ok(())
}

fn encode(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn deletion_date() -> String {
    // SAFETY: localtime_r writes only into the zeroed tm we pass.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

/// Moves `path` into its volume's trash and records where it came from.
pub fn trash(path: &Path) -> io::Result<Stored> {
    trash_with(path, home_trash())
}

/// `trash` with an explicit home trash location, so tests never touch the real one.
pub fn trash_with(path: &Path, home: Option<PathBuf>) -> io::Result<Stored> {
    let original = path.canonicalize_parent()?;
    store(&original, home, |destination| {
        rename_noreplace(libc::AT_FDCWD, &original, destination)
    })
}

/// Reserve a destination and record the original path. The caller can move a file
/// through a pinned directory descriptor after its own identity checks.
pub fn store(
    original: &Path,
    home: Option<PathBuf>,
    move_file: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<Stored> {
    let dir = trash_dir(original, home)?;
    if original.starts_with(&dir) {
        return Err(io::Error::other("This item is already in Trash."));
    }
    ensure(&dir)?;
    let name = original
        .file_name()
        .ok_or_else(|| io::Error::other("Cannot trash this item."))?
        .to_string_lossy()
        .into_owned();
    // Leave room for the collision suffix and .trashinfo, even for a maximum-length file name.
    let mut end = name.len().min(216);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let name = &name[..end];
    // The info file is the reservation: create_new fails if another item already uses the name.
    let (info, mut writer) = (1..)
        .find_map(|n| {
            let candidate = if n == 1 {
                name.to_owned()
            } else {
                format!("{name}.{n}")
            };
            if fs::symlink_metadata(dir.join("files").join(&candidate)).is_ok() {
                return None;
            }
            let info = dir.join("info").join(format!("{candidate}.trashinfo"));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&info)
            {
                Ok(file) => Some(Ok((info, file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .expect("an unbounded search")?;
    let stored_name = info.file_stem().unwrap().to_owned();
    let file = dir.join("files").join(stored_name);
    let written = write!(
        writer,
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encode(original),
        deletion_date()
    );
    let moved = written.and_then(|()| move_file(&file));
    if let Err(error) = moved {
        let _ = fs::remove_file(&info);
        return Err(error);
    }
    Ok(Stored {
        original: original.to_path_buf(),
        file,
        info,
    })
}

/// Puts a trashed item back where it was. Never overwrites: the original name must be free.
pub fn restore(stored: &Stored) -> io::Result<()> {
    if fs::symlink_metadata(&stored.original).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("“{}” already exists.", stored.original.display()),
        ));
    }
    if let Some(parent) = stored.original.parent() {
        fs::create_dir_all(parent)?;
    }
    rename_noreplace(libc::AT_FDCWD, &stored.file, &stored.original)?;
    let _ = fs::remove_file(&stored.info);
    Ok(())
}

pub fn rename_noreplace(from_dir: i32, from: &Path, to: &Path) -> io::Result<()> {
    let path = |value: &Path| CString::new(value.as_os_str().as_bytes()).map_err(io::Error::other);
    let from = path(from)?;
    let to = path(to)?;
    // SAFETY: both C strings remain valid for the syscall.
    if unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            from_dir,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

trait ParentCanonical {
    /// Resolves the folder but not the item itself, so trashing a symlink trashes the link.
    fn canonicalize_parent(&self) -> io::Result<PathBuf>;
}
impl ParentCanonical for Path {
    fn canonicalize_parent(&self) -> io::Result<PathBuf> {
        let name = self
            .file_name()
            .ok_or_else(|| io::Error::other("Cannot trash the root folder."))?;
        let parent = self
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Ok(parent.canonicalize()?.join(name))
    }
}
