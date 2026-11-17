//! Reading directories and searching below them. Runs on the worker thread, never the UI thread.
use super::entry::Entry;
use std::{
    cmp::Ordering,
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering as Atomic},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Modified,
    Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self { key: SortKey::Name, ascending: true }
    }
}

impl Sort {
    /// Choosing the active column again reverses it; a new column starts in its natural direction.
    pub fn choose(&mut self, key: SortKey) {
        if self.key == key {
            self.ascending = !self.ascending;
        } else {
            *self = Self { key, ascending: matches!(key, SortKey::Name | SortKey::Type) };
        }
    }
}

/// Search results stop here so a broad query cannot exhaust memory.
pub const SEARCH_LIMIT: usize = 5_000;

pub fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let reader = fs::read_dir(dir).map_err(|e| describe(&e, dir))?;
    let mut entries = Vec::new();
    for item in reader.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        // DirEntry::metadata does not follow links, which keeps the common case to one lstat.
        let Ok(meta) = item.metadata() else { continue };
        let link = meta.file_type().is_symlink();
        let entry = if link {
            Entry::read(&item.path(), name)
        } else {
            Some(Entry::from_metadata(name, false, &meta))
        };
        entries.extend(entry);
    }
    Ok(entries)
}

/// Case-insensitive name search below `root`. Symlinked folders are listed but not entered, so a
/// link loop cannot run forever. `generation` ends the walk when a newer request replaces it.
pub fn search(root: &Path, query: &str, request: u64, generation: &AtomicU64) -> Vec<Entry> {
    let needle = query.to_lowercase();
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        if generation.load(Atomic::Relaxed) != request || found.len() >= SEARCH_LIMIT {
            break;
        }
        let Ok(reader) = fs::read_dir(&dir) else { continue };
        for item in reader.flatten() {
            let leaf = item.file_name().to_string_lossy().into_owned();
            let Ok(meta) = item.metadata() else { continue };
            let link = meta.file_type().is_symlink();
            let name = format!("{prefix}{leaf}");
            let matches = leaf.to_lowercase().contains(&needle);
            let entry = if link {
                Entry::read(&item.path(), name.clone())
            } else {
                Some(Entry::from_metadata(name.clone(), false, &meta))
            };
            let Some(entry) = entry else { continue };
            if entry.is_dir() && !link {
                pending.push((item.path(), format!("{name}/")));
            }
            if matches && found.len() < SEARCH_LIMIT {
                found.push(entry);
            }
        }
    }
    found
}

fn describe(error: &std::io::Error, dir: &Path) -> String {
    use std::io::ErrorKind::*;
    let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
    match error.kind() {
        NotFound => format!("“{name}” no longer exists."),
        PermissionDenied => format!("You do not have permission to open “{name}”."),
        NotADirectory => format!("“{name}” is not a folder."),
        _ => format!("Cannot open “{name}”: {error}"),
    }
}

/// Case-insensitive comparison that orders digit runs by value: "file2" before "file10".
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (nx, ny) = (digits(&mut a), digits(&mut b));
                let (tx, ty) = (nx.trim_start_matches('0'), ny.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

fn digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut run = String::new();
    while let Some(c) = chars.next_if(|c| c.is_ascii_digit()) {
        run.push(c);
    }
    run
}

/// Folders first, then by the chosen column with the name as the tie-breaker.
pub fn compare(a: &Entry, b: &Entry, sort: Sort) -> Ordering {
    let by_kind = b.is_dir().cmp(&a.is_dir());
    if by_kind != Ordering::Equal {
        return by_kind;
    }
    let column = match sort.key {
        SortKey::Name => natural(&a.name, &b.name),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Modified => a.modified.cmp(&b.modified),
        SortKey::Type => a.extension().cmp(&b.extension()),
    };
    let column = if sort.ascending { column } else { column.reverse() };
    column.then_with(|| natural(&a.name, &b.name))
}
