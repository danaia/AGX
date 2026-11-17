//! The rows Slint shows for one folder. Rows are formatted on request, so only what is on screen
//! costs anything, and selection changes notify only the rows whose appearance changed.
use super::{
    entry::{Entry, size_text, time_text},
    listing::{Sort, compare},
};
use crate::FileRow;
use slint::{Model, ModelNotify, ModelTracker};
use std::{cell::RefCell, path::PathBuf};

/// How the entries are ordered and which are shown.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub sort: Sort,
    /// Lower-case text a name must contain.
    pub filter: String,
    pub show_hidden: bool,
    pub folders_only: bool,
}

#[derive(Default)]
struct Inner {
    entries: Vec<Entry>,
    /// Visible rows, as indices into `entries`.
    order: Vec<u32>,
    /// By entry index. Entries that are not visible are never selected.
    selected: Vec<bool>,
    cursor: Option<usize>,
    anchor: Option<usize>,
}

#[derive(Default)]
pub struct FileModel {
    inner: RefCell<Inner>,
    notify: ModelNotify,
}

/// Counts for the status bar.
#[derive(Debug, Default)]
pub struct Stats {
    pub visible: usize,
    pub folders: usize,
    pub selected: usize,
    pub selected_bytes: u64,
    /// The selected entry, when exactly one is selected.
    pub single: Option<Entry>,
}

impl FileModel {
    /// Replaces the entries. `keep` names stay selected, and the first of them gets the cursor.
    pub fn replace(&self, entries: Vec<Entry>, view: &View, keep: &[String]) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.selected = vec![false; entries.len()];
            inner.entries = entries;
            inner.cursor = None;
            inner.anchor = None;
            inner.rebuild(view);
            let mut first = None;
            let order = inner.order.clone();
            for (row, &index) in order.iter().enumerate() {
                if keep.contains(&inner.entries[index as usize].name) {
                    inner.selected[index as usize] = true;
                    first.get_or_insert(row);
                }
            }
            inner.cursor = first;
            inner.anchor = first;
        }
        self.notify.reset();
    }

    /// Applies new sorting or filtering. The cursor and selection follow their entries.
    pub fn restyle(&self, view: &View) {
        {
            let mut inner = self.inner.borrow_mut();
            let cursor = inner.cursor.and_then(|row| inner.order.get(row).copied());
            inner.rebuild(view);
            inner.cursor = cursor.and_then(|e| inner.order.iter().position(|&i| i == e));
            inner.anchor = inner.cursor;
        }
        self.notify.reset();
    }

    pub fn len(&self) -> usize {
        self.inner.borrow().order.len()
    }

    pub fn entry(&self, row: usize) -> Option<Entry> {
        let inner = self.inner.borrow();
        inner.order.get(row).map(|&i| inner.entries[i as usize].clone())
    }

    pub fn cursor(&self) -> Option<usize> {
        self.inner.borrow().cursor
    }

    /// Selected entries, in display order.
    pub fn selected(&self) -> Vec<Entry> {
        let inner = self.inner.borrow();
        inner.order.iter().filter(|&&i| inner.selected[i as usize]).map(|&i| inner.entries[i as usize].clone()).collect()
    }

    pub fn stats(&self) -> Stats {
        let inner = self.inner.borrow();
        let mut stats = Stats { visible: inner.order.len(), ..Stats::default() };
        let mut first = None;
        for &i in &inner.order {
            let entry = &inner.entries[i as usize];
            stats.folders += usize::from(entry.is_dir());
            if inner.selected[i as usize] {
                stats.selected += 1;
                stats.selected_bytes += entry.size;
                first.get_or_insert(i as usize);
            }
        }
        if stats.selected == 1 {
            stats.single = first.map(|i| inner.entries[i].clone());
        }
        stats
    }

    /// First row at or after `from` (wrapping) whose name starts with `prefix`, ignoring case.
    pub fn find_prefix(&self, prefix: &str, from: usize) -> Option<usize> {
        let inner = self.inner.borrow();
        let prefix = prefix.to_lowercase();
        let n = inner.order.len();
        (0..n).map(|k| (from + k) % n).find(|&row| inner.entries[inner.order[row] as usize].name.to_lowercase().starts_with(&prefix))
    }

    pub fn paths(&self, root: &std::path::Path) -> Vec<PathBuf> {
        self.selected().iter().map(|e| root.join(&e.name)).collect()
    }

    // --- selection -----------------------------------------------------------------------

    /// A mouse click: plain selects one row, Ctrl toggles it, Shift extends from the anchor.
    pub fn click(&self, row: usize, ctrl: bool, shift: bool) {
        self.change(|inner| {
            if row >= inner.order.len() {
                return;
            }
            if shift {
                inner.select_range(inner.anchor.unwrap_or(row), row, ctrl);
            } else {
                if !ctrl {
                    inner.selected.fill(false);
                }
                let index = inner.order[row] as usize;
                inner.selected[index] = !(ctrl && inner.selected[index]);
                inner.anchor = Some(row);
            }
            inner.cursor = Some(row);
        });
    }

    /// Keyboard movement. With `extend`, the range from the anchor is selected.
    pub fn move_cursor(&self, row: usize, extend: bool) {
        self.change(|inner| {
            if inner.order.is_empty() {
                return;
            }
            let row = row.min(inner.order.len() - 1);
            if extend {
                inner.select_range(inner.anchor.unwrap_or(row), row, false);
            } else {
                inner.selected.fill(false);
                inner.selected[inner.order[row] as usize] = true;
                inner.anchor = Some(row);
            }
            inner.cursor = Some(row);
        });
    }

    /// Moves the cursor without changing the selection (Ctrl+arrows).
    pub fn focus_row(&self, row: usize) {
        self.change(|inner| {
            if !inner.order.is_empty() {
                inner.cursor = Some(row.min(inner.order.len() - 1));
            }
        });
    }

    pub fn toggle_cursor(&self) {
        self.change(|inner| {
            if let Some(&i) = inner.cursor.and_then(|row| inner.order.get(row)) {
                inner.selected[i as usize] ^= true;
            }
        });
    }

    pub fn select_all(&self) {
        self.change(|inner| {
            for i in inner.order.clone() {
                inner.selected[i as usize] = true;
            }
        });
    }

    pub fn clear(&self) {
        self.change(|inner| inner.selected.fill(false));
    }

    /// Runs a selection change, then notifies exactly the rows whose look changed.
    fn change(&self, edit: impl FnOnce(&mut Inner)) {
        let (before, old_cursor) = {
            let inner = self.inner.borrow();
            (inner.order.iter().map(|&i| inner.selected[i as usize]).collect::<Vec<_>>(), inner.cursor)
        };
        let dirty: Vec<usize> = {
            let mut inner = self.inner.borrow_mut();
            edit(&mut inner);
            let mut rows: Vec<usize> =
                (0..before.len()).filter(|&row| inner.selected[inner.order[row] as usize] != before[row]).collect();
            rows.extend([old_cursor, inner.cursor].into_iter().flatten().filter(|&r| r < before.len()));
            rows.sort_unstable();
            rows.dedup();
            rows
        };
        if dirty.len() > 256 {
            self.notify.reset();
        } else {
            for row in dirty {
                self.notify.row_changed(row);
            }
        }
    }
}

impl Inner {
    fn rebuild(&mut self, view: &View) {
        let visible = |e: &Entry| {
            (view.show_hidden || !e.hidden())
                && (!view.folders_only || e.is_dir())
                && (view.filter.is_empty() || e.name.to_lowercase().contains(&view.filter))
        };
        let mut order: Vec<u32> = (0..self.entries.len() as u32).filter(|&i| visible(&self.entries[i as usize])).collect();
        order.sort_by(|&a, &b| compare(&self.entries[a as usize], &self.entries[b as usize], view.sort));
        let mut shown = vec![false; self.entries.len()];
        order.iter().for_each(|&i| shown[i as usize] = true);
        for (flag, shown) in self.selected.iter_mut().zip(shown) {
            *flag &= shown;
        }
        self.order = order;
    }

    fn select_range(&mut self, from: usize, to: usize, keep: bool) {
        if !keep {
            self.selected.fill(false);
        }
        let (low, high) = (from.min(to), from.max(to));
        for row in low..=high.min(self.order.len().saturating_sub(1)) {
            self.selected[self.order[row] as usize] = true;
        }
        self.anchor = Some(from);
    }
}

impl Model for FileModel {
    type Data = FileRow;

    fn row_count(&self) -> usize {
        self.inner.borrow().order.len()
    }

    fn row_data(&self, row: usize) -> Option<FileRow> {
        let inner = self.inner.borrow();
        let index = *inner.order.get(row)? as usize;
        let entry = &inner.entries[index];
        let (glyph, tone) = entry.glyph();
        Some(FileRow {
            name: entry.name.as_str().into(),
            glyph: glyph.into(),
            tone,
            size: if entry.is_dir() { "".into() } else { size_text(entry.size).into() },
            modified: time_text(entry.modified).into(),
            kind: entry.type_label().into(),
            selected: inner.selected[index],
            cursor: inner.cursor == Some(row),
            dir: entry.is_dir(),
            dim: entry.hidden(),
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}
