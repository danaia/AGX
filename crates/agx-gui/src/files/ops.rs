//! File operations and their undo records. Nothing here overwrites an existing file: a name that is
//! taken gets a " (copy)" suffix instead, so a paste can never destroy data.
use super::trash::{self, Stored};
use std::{
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub enum Op {
    Copy { items: Vec<PathBuf>, into: PathBuf },
    Move { items: Vec<PathBuf>, into: PathBuf },
    Trash { items: Vec<PathBuf> },
    Delete { items: Vec<PathBuf> },
    Create { dir: PathBuf, name: String, folder: bool },
    Rename { path: PathBuf, name: String },
    Undo(Action),
    Redo(Action),
}

/// What was done, with enough detail to reverse or repeat it.
#[derive(Clone, Debug)]
pub enum Action {
    Created { path: PathBuf, folder: bool },
    Renamed { from: PathBuf, to: PathBuf },
    Moved { pairs: Vec<(PathBuf, PathBuf)> },
    Copied { pairs: Vec<(PathBuf, PathBuf)> },
    Trashed { items: Vec<Stored> },
}

impl Action {
    fn len(&self) -> usize {
        match self {
            Self::Created { .. } | Self::Renamed { .. } => 1,
            Self::Moved { pairs } | Self::Copied { pairs } => pairs.len(),
            Self::Trashed { items } => items.len(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// Sentence for the status line, e.g. "Copied 3 items".
    pub summary: String,
    /// What to push on the undo stack; `None` for permanent or failed work.
    pub action: Option<Action>,
    /// Final paths of created items, so the view can select them.
    pub results: Vec<PathBuf>,
    pub errors: Vec<String>,
}

/// Progress for long operations: items finished, total, and the item being worked on.
pub type Progress<'a> = &'a mut dyn FnMut(usize, usize, &str);

pub fn run(op: Op, progress: Progress) -> Outcome {
    match op {
        Op::Copy { items, into } => transfer(items, &into, false, progress),
        Op::Move { items, into } => transfer(items, &into, true, progress),
        Op::Trash { items } => trash_items(items, progress),
        Op::Delete { items } => delete_items(items, progress),
        Op::Create { dir, name, folder } => create(&dir, &name, folder),
        Op::Rename { path, name } => rename(&path, &name),
        Op::Undo(action) => reverse(action, true),
        Op::Redo(action) => reverse(action, false),
    }
}

pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        Err("Enter a name.".into())
    } else if name.contains('/') || name.contains('\0') {
        Err("A name cannot contain “/”.".into())
    } else if name.len() > 255 {
        Err("That name is too long.".into())
    } else {
        Ok(())
    }
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// `name` if free in `dir`, else "name (copy).ext", "name (copy 2).ext", …
pub fn unique_name(dir: &Path, name: &str) -> String {
    if !exists(&dir.join(name)) {
        return name.to_owned();
    }
    let (stem, ext) = split_extension(name);
    (1..)
        .map(|n| if n == 1 { format!("{stem} (copy){ext}") } else { format!("{stem} (copy {n}){ext}") })
        .find(|candidate| !exists(&dir.join(candidate)))
        .expect("an unbounded search")
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(dot) => {
            let dot = if name[..dot].ends_with(".tar") { dot - 4 } else { dot };
            name.split_at(dot)
        }
    }
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 { format!("1 {word}") } else { format!("{count} {word}s") }
}

fn leaf(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn failure(path: &Path, error: &io::Error) -> String {
    let reason = match error.kind() {
        io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
        io::ErrorKind::NotFound => "no longer exists".to_owned(),
        _ => error.to_string(),
    };
    format!("“{}”: {reason}", leaf(path))
}

fn summarize(done: usize, verb: &str, errors: &[String]) -> String {
    match (done, errors.first()) {
        (0, Some(first)) => format!("Could not {verb}: {first}"),
        (_, Some(first)) => format!("{} done, {} failed: {first}", plural(done, "item"), errors.len()),
        _ => String::new(),
    }
}

// --- copy and move ---------------------------------------------------------------------------

fn transfer(items: Vec<PathBuf>, into: &Path, moving: bool, progress: Progress) -> Outcome {
    let mut out = Outcome::default();
    let mut pairs = Vec::new();
    let total = items.len();
    for (n, from) in items.iter().enumerate() {
        progress(n, total, &leaf(from));
        match transfer_one(from, into, moving) {
            Ok(Some(to)) => pairs.push((from.clone(), to)),
            Ok(None) => {}
            Err(e) => out.errors.push(match e {
                Refused::Message(m) => m,
                Refused::Io(e) => failure(from, &e),
            }),
        }
    }
    out.results = pairs.iter().map(|(_, to)| to.clone()).collect();
    let verb = if moving { "move" } else { "copy" };
    out.summary = if out.errors.is_empty() {
        format!("{} {}", if moving { "Moved" } else { "Copied" }, plural(pairs.len(), "item"))
    } else {
        summarize(pairs.len(), verb, &out.errors)
    };
    if !pairs.is_empty() {
        out.action = Some(if moving { Action::Moved { pairs } } else { Action::Copied { pairs } });
    }
    out
}

enum Refused {
    Message(String),
    Io(io::Error),
}
impl From<io::Error> for Refused {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn transfer_one(from: &Path, into: &Path, moving: bool) -> Result<Option<PathBuf>, Refused> {
    let name = from.file_name().ok_or_else(|| Refused::Message("Cannot use the root folder.".into()))?.to_string_lossy().into_owned();
    let meta = fs::symlink_metadata(from)?;
    if meta.is_dir() && into.starts_with(from) {
        return Err(Refused::Message(format!("“{name}” cannot be placed inside itself.")));
    }
    if moving && from.parent() == Some(into) {
        return Ok(None);
    }
    let to = into.join(unique_name(into, &name));
    if moving {
        move_path(from, &to)?;
    } else {
        copy_tree(from, &to)?;
    }
    Ok(Some(to))
}

/// A rename, or copy-then-remove when the destination is on another volume.
fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            copy_tree(from, to)?;
            remove_tree(from)
        }
        other => other,
    }
}

/// Recursive copy that keeps symlinks as links and never follows them.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        symlink(fs::read_link(from)?, to)
    } else if meta.is_dir() {
        fs::create_dir(to)?;
        let result = fs::read_dir(from)?.try_for_each(|item| {
            let item = item?;
            copy_tree(&item.path(), &to.join(item.file_name()))
        });
        // Permissions last, so a read-only source folder is still writable while filling it.
        result.and_then(|()| fs::set_permissions(to, meta.permissions()))
    } else if meta.is_file() {
        fs::copy(from, to).map(drop)
    } else {
        Err(io::Error::other("special files cannot be copied"))
    }
}

fn remove_tree(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) }
}

// --- trash and delete ------------------------------------------------------------------------

fn trash_items(items: Vec<PathBuf>, progress: Progress) -> Outcome {
    let mut out = Outcome::default();
    let mut stored = Vec::new();
    let total = items.len();
    for (n, path) in items.iter().enumerate() {
        progress(n, total, &leaf(path));
        match trash::trash(path) {
            Ok(s) => stored.push(s),
            Err(e) => out.errors.push(failure(path, &e)),
        }
    }
    out.summary = if out.errors.is_empty() {
        format!("Moved {} to Trash", plural(stored.len(), "item"))
    } else {
        summarize(stored.len(), "move to Trash", &out.errors)
    };
    if !stored.is_empty() {
        out.action = Some(Action::Trashed { items: stored });
    }
    out
}

fn delete_items(items: Vec<PathBuf>, progress: Progress) -> Outcome {
    let mut out = Outcome::default();
    let (mut done, total) = (0, items.len());
    for (n, path) in items.iter().enumerate() {
        progress(n, total, &leaf(path));
        match path.parent().map(|_| remove_tree(path)).unwrap_or_else(|| Err(io::Error::other("root"))) {
            Ok(()) => done += 1,
            Err(e) => out.errors.push(failure(path, &e)),
        }
    }
    out.summary =
        if out.errors.is_empty() { format!("Deleted {}", plural(done, "item")) } else { summarize(done, "delete", &out.errors) };
    out
}

// --- create and rename -----------------------------------------------------------------------

fn create(dir: &Path, name: &str, folder: bool) -> Outcome {
    let mut out = Outcome::default();
    if let Err(message) = valid_name(name) {
        out.summary = message;
        return out;
    }
    let path = dir.join(name);
    let made = if folder { fs::create_dir(&path) } else { fs::OpenOptions::new().write(true).create_new(true).open(&path).map(drop) };
    match made {
        Ok(()) => {
            out.summary = format!("Created “{name}”");
            out.results = vec![path.clone()];
            out.action = Some(Action::Created { path, folder });
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => out.summary = format!("“{name}” already exists."),
        Err(e) => out.summary = format!("Could not create “{name}”: {}", failure(&path, &e)),
    }
    out
}

fn rename(path: &Path, name: &str) -> Outcome {
    let mut out = Outcome::default();
    if let Err(message) = valid_name(name) {
        out.summary = message;
        return out;
    }
    let to = path.with_file_name(name);
    if to == path {
        return out;
    }
    if exists(&to) {
        out.summary = format!("“{name}” already exists.");
        return out;
    }
    match fs::rename(path, &to) {
        Ok(()) => {
            out.summary = format!("Renamed to “{name}”");
            out.results = vec![to.clone()];
            out.action = Some(Action::Renamed { from: path.to_path_buf(), to });
        }
        Err(e) => out.summary = format!("Could not rename: {}", failure(path, &e)),
    }
    out
}

// --- undo and redo ---------------------------------------------------------------------------

/// Reverses (`undo`) or repeats an action. The outcome's action is the record for the other stack.
fn reverse(action: Action, undo: bool) -> Outcome {
    let mut out = Outcome::default();
    let verb = if undo { "Undo" } else { "Redo" };
    let items = action.len();
    let result = match action {
        Action::Created { ref path, folder } if undo => {
            let removed = if folder { fs::remove_dir(path) } else { fs::remove_file(path) };
            removed.map_err(|e| failure(path, &e)).map(|()| (format!("Removed “{}”", leaf(path)), action.clone(), vec![]))
        }
        Action::Created { ref path, folder } => {
            let made = if folder { fs::create_dir(path) } else { fs::OpenOptions::new().write(true).create_new(true).open(path).map(drop) };
            made.map_err(|e| failure(path, &e)).map(|()| (format!("Created “{}”", leaf(path)), action.clone(), vec![path.clone()]))
        }
        Action::Renamed { ref from, ref to } => {
            let (src, dst) = if undo { (to, from) } else { (from, to) };
            relocate(src, dst).map(|()| (format!("Renamed to “{}”", leaf(dst)), action.clone(), vec![dst.clone()]))
        }
        Action::Moved { ref pairs } => {
            let mut done = Vec::new();
            for (from, to) in pairs {
                let (src, dst) = if undo { (to, from) } else { (from, to) };
                if let Err(e) = relocate(src, dst) {
                    out.errors.push(e);
                } else {
                    done.push(dst.clone());
                }
            }
            Ok((format!("{} {}", if undo { "Moved back" } else { "Moved" }, plural(done.len(), "item")), action.clone(), done))
        }
        Action::Copied { ref pairs } if undo => {
            // The copies go to Trash rather than being erased, in case they were edited.
            let mut stored = Vec::new();
            for (_, to) in pairs {
                match trash::trash(to) {
                    Ok(s) => stored.push(s),
                    Err(e) => out.errors.push(failure(to, &e)),
                }
            }
            Ok((format!("Removed {}", plural(stored.len(), "copy")), action.clone(), vec![]))
        }
        Action::Copied { ref pairs } => {
            let mut again = Vec::new();
            for (from, to) in pairs {
                let target = to.with_file_name(unique_name(to.parent().unwrap_or(Path::new("/")), &leaf(to)));
                match copy_tree(from, &target) {
                    Ok(()) => again.push((from.clone(), target)),
                    Err(e) => out.errors.push(failure(from, &e)),
                }
            }
            let results = again.iter().map(|(_, t)| t.clone()).collect();
            Ok((format!("Copied {}", plural(again.len(), "item")), Action::Copied { pairs: again }, results))
        }
        Action::Trashed { ref items } if undo => {
            let mut restored = Vec::new();
            for item in items {
                match trash::restore(item) {
                    Ok(()) => restored.push(item.original.clone()),
                    Err(e) => out.errors.push(failure(&item.original, &e)),
                }
            }
            Ok((format!("Restored {}", plural(restored.len(), "item")), action.clone(), restored))
        }
        Action::Trashed { ref items } => {
            let mut stored = Vec::new();
            for item in items {
                match trash::trash(&item.original) {
                    Ok(s) => stored.push(s),
                    Err(e) => out.errors.push(failure(&item.original, &e)),
                }
            }
            Ok((format!("Moved {} to Trash", plural(stored.len(), "item")), Action::Trashed { items: stored }, vec![]))
        }
    };
    match result {
        // Every item failed: nothing changed, so there is nothing to put on either stack.
        Ok(_) if out.errors.len() >= items => {
            out.summary = format!("{verb} failed: {}", out.errors[0]);
        }
        Ok((summary, record, results)) => {
            out.summary = if let Some(first) = out.errors.first() { format!("{summary}; {first}") } else { summary };
            out.results = results;
            out.action = Some(record);
        }
        Err(message) => out.summary = format!("{verb} failed: {message}"),
    }
    out
}

/// A rename that refuses to replace anything.
fn relocate(from: &Path, to: &Path) -> Result<(), String> {
    if exists(to) {
        return Err(format!("“{}” already exists.", leaf(to)));
    }
    move_path(from, to).map_err(|e| failure(from, &e))
}
