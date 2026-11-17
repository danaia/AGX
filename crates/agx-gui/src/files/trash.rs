//! Moving files to the freedesktop.org Trash, so deleting is recoverable.
//!
//! Files on the home volume go to `$XDG_DATA_HOME/Trash`; files on any other volume go to
//! `<volume>/.Trash-<uid>`. Keeping each file on its own volume makes trashing a rename, never a
//! copy, which matters for large folders on the NVMe data volume.
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

/// Where a trashed file lives, enough to put it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub original: PathBuf,
    pub file: PathBuf,
    pub info: PathBuf,
}

fn home_trash() -> Option<PathBuf> {
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
    let parent = path.parent().ok_or_else(|| io::Error::other("Cannot trash the root folder."))?;
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
    // SAFETY: geteuid has no preconditions.
    Ok(top.join(format!(".Trash-{}", unsafe { libc::geteuid() })))
}

fn ensure(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir.join("files"))?;
    builder.create(dir.join("info"))
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
    let dir = trash_dir(&original, home)?;
    ensure(&dir)?;
    let name = original.file_name().ok_or_else(|| io::Error::other("Cannot trash this item."))?.to_string_lossy().into_owned();
    // The info file is the reservation: create_new fails if another item already uses the name.
    let (info, mut writer) = (1..)
        .find_map(|n| {
            let candidate = if n == 1 { name.clone() } else { format!("{name}.{n}") };
            let info = dir.join("info").join(format!("{candidate}.trashinfo"));
            match fs::OpenOptions::new().write(true).create_new(true).open(&info) {
                Ok(file) => Some(Ok((info, file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .expect("an unbounded search")?;
    let stored_name = info.file_stem().unwrap().to_owned();
    let file = dir.join("files").join(stored_name);
    let written = write!(writer, "[Trash Info]\nPath={}\nDeletionDate={}\n", encode(&original), deletion_date());
    let moved = written.and_then(|()| fs::rename(&original, &file));
    if let Err(error) = moved {
        let _ = fs::remove_file(&info);
        return Err(error);
    }
    Ok(Stored { original, file, info })
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
    fs::rename(&stored.file, &stored.original)?;
    let _ = fs::remove_file(&stored.info);
    Ok(())
}

trait ParentCanonical {
    /// Resolves the folder but not the item itself, so trashing a symlink trashes the link.
    fn canonicalize_parent(&self) -> io::Result<PathBuf>;
}
impl ParentCanonical for Path {
    fn canonicalize_parent(&self) -> io::Result<PathBuf> {
        let name = self.file_name().ok_or_else(|| io::Error::other("Cannot trash the root folder."))?;
        let parent = self.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        Ok(parent.canonicalize()?.join(name))
    }
}
