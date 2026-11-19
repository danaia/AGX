//! Publishes the bookmark bar and its cascading folder menus to the Web chrome, and handles
//! import, star and menu commands. The bar mirrors Chrome: links and folders that fit, a `»`
//! overflow folder for the rest, and an "All Bookmarks" folder holding the three Chrome roots.
use super::bookmarks::{self, Bar, Entry};
use super::favicons::Icons;
use crate::{App, AppWindow, BookmarkItem, BookmarkRow, Web};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

const DEFAULT_WIDTH: f32 = 1200.0;
const BAR_PADDING: f32 = 12.0;
const GAP: f32 = 2.0;

fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("");
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

fn label(entry: &Entry) -> String {
    match (&entry.url, entry.title.is_empty()) {
        (Some(url), true) => host(url),
        _ => entry.title.clone(),
    }
}

/// A link with no title: the bar shows only its icon (or a letter badge when there is no icon).
fn compact(entry: &Entry) -> bool {
    entry.url.is_some() && entry.title.is_empty()
}

fn icon(icons: &mut Icons, entry: &Entry) -> Option<slint::Image> {
    icons.get(&host(entry.url.as_deref()?))
}

/// First letter of an untitled link without an icon, shown as a small badge.
fn letter(entry: &Entry, has_icon: bool) -> String {
    if compact(entry) && !has_icon {
        label(entry).chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "•".into())
    } else {
        String::new()
    }
}

/// Estimated pixel width of a bar button; mirrors the Slint sizing (12px text, 8px padding).
fn estimate(entry: &Entry, icons: &mut Icons) -> f32 {
    let width = if compact(entry) {
        30.0
    } else {
        let chars = entry.title.chars().count() + if entry.url.is_none() { 2 } else { 0 };
        let padding = if icon(icons, entry).is_some() { 36.0 } else { 16.0 };
        (chars as f32 * 7.0 + padding).min(170.0)
    };
    width + GAP
}

/// The buttons for a bar `width` pixels wide, in order: fitting items, `»`, then "All Bookmarks".
fn layout(bar: &Bar, width: f32, icons: &mut Icons) -> Vec<Entry> {
    let mut all = Vec::new();
    for (title, children) in [
        ("Bookmarks bar", &bar.items),
        ("Other bookmarks", &bar.other),
        ("Mobile bookmarks", &bar.mobile),
    ] {
        if !children.is_empty() {
            all.push(Entry { title: title.into(), url: None, children: children.clone() });
        }
    }
    let all = (!all.is_empty()).then(|| Entry { title: "All Bookmarks".into(), url: None, children: all });
    let reserved = all.as_ref().map_or(0.0, |all| estimate(all, icons) + 16.0);
    let room = (width - BAR_PADDING - reserved).max(0.0);
    let total: f32 = bar.items.iter().map(|item| estimate(item, icons)).sum();
    let mut shown = bar.items.len();
    if total > room {
        let (mut used, room) = (0.0, room - 32.0);
        shown = bar.items.iter().take_while(|item| {
            used += estimate(item, icons);
            used <= room
        }).count();
    }
    let mut out: Vec<Entry> = bar.items[..shown].to_vec();
    if shown < bar.items.len() {
        out.push(Entry { title: "»".into(), url: None, children: bar.items[shown..].to_vec() });
    }
    out.extend(all);
    out
}

fn resolve<'a>(display: &'a [Entry], path: &[usize]) -> Option<&'a Entry> {
    let mut entry = display.get(*path.first()?)?;
    for &index in &path[1..] {
        entry = entry.children.get(index)?;
    }
    Some(entry)
}

impl App {
    fn bookmarks_publish(&mut self, web: &Web<'_>) {
        let browser = &mut self.browser;
        let last = browser.bookmark_display.len().saturating_sub(1);
        let all = !browser.bookmarks.items.is_empty() || !browser.bookmarks.other.is_empty() || !browser.bookmarks.mobile.is_empty();
        let items: Vec<BookmarkItem> = browser
            .bookmark_display
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let image = icon(&mut browser.icons, item);
                BookmarkItem {
                    title: label(item).into(),
                    letter: letter(item, image.is_some()).into(),
                    has_icon: image.is_some(),
                    icon: image.unwrap_or_default(),
                    compact: compact(item),
                    url: item.url.clone().unwrap_or_default().into(),
                    folder: item.url.is_none(),
                    pinned: all && i == last,
                    open: browser.bookmark_path.first() == Some(&i),
                }
            })
            .collect();
        web.set_bookmarks(ModelRc::new(VecModel::from(items)));
        let menus: Vec<ModelRc<BookmarkRow>> = (0..browser.bookmark_path.len())
            .filter_map(|level| {
                let folder = resolve(&browser.bookmark_display, &browser.bookmark_path[..=level])?;
                let next = browser.bookmark_path.get(level + 1);
                let rows: Vec<BookmarkRow> = folder
                    .children
                    .iter()
                    .enumerate()
                    .map(|(r, child)| {
                        let image = icon(&mut browser.icons, child);
                        BookmarkRow {
                            title: label(child).into(),
                            has_icon: image.is_some(),
                            icon: image.unwrap_or_default(),
                            url: child.url.clone().unwrap_or_default().into(),
                            folder: child.url.is_none(),
                            open: next == Some(&r),
                        }
                    })
                    .collect();
                Some(ModelRc::new(VecModel::from(rows)))
            })
            .collect();
        web.set_menus(ModelRc::new(VecModel::from(menus)));
        web.set_starred(browser.bookmarks.contains(&browser.uri));
    }

    /// Recomputes which buttons fit and closes any open menu.
    fn bookmarks_relayout(&mut self, ui: &AppWindow) {
        let web = ui.global::<Web>();
        let width = match web.get_bar_width() {
            0 => DEFAULT_WIDTH,
            width => width as f32,
        };
        self.browser.bookmark_width = web.get_bar_width();
        self.browser.bookmark_display = layout(&self.browser.bookmarks, width, &mut self.browser.icons);
        self.browser.bookmark_path.clear();
        self.bookmarks_publish(&web);
    }

    pub(super) fn bookmarks_load(&mut self, ui: &AppWindow) {
        self.browser.icons = Icons::new();
        if let Some(bar) = bookmarks::store_path().and_then(|path| Bar::load(&path)) {
            self.browser.bookmarks = bar;
        }
        self.bookmarks_relayout(ui);
    }

    /// Re-fits the bar when the window width changes.
    fn bookmarks_resize(&mut self, ui: &AppWindow) {
        if ui.global::<Web>().get_bar_width() != self.browser.bookmark_width {
            self.bookmarks_relayout(ui);
        }
    }

    pub(super) fn bookmark_folder(&mut self, ui: &AppWindow, level: i32, index: i32) {
        let Ok(index) = usize::try_from(index) else { return };
        let path = &mut self.browser.bookmark_path;
        match usize::try_from(level) {
            Err(_) if path.as_slice() == [index] => path.clear(),
            Err(_) => *path = vec![index],
            Ok(level) if level < path.len() => {
                path.truncate(level + 1);
                let folder = resolve(&self.browser.bookmark_display, path);
                if folder.and_then(|f| f.children.get(index)).is_some_and(|c| c.url.is_none()) {
                    self.browser.bookmark_path.push(index);
                }
            }
            Ok(_) => {}
        }
        self.bookmarks_publish(&ui.global::<Web>());
    }

    pub(super) fn bookmark_open(&mut self, ui: &AppWindow, address: &str) {
        self.bookmark_close(ui);
        self.browser_navigate(ui, address);
    }

    pub(super) fn bookmark_close(&mut self, ui: &AppWindow) {
        if !self.browser.bookmark_path.is_empty() {
            self.browser.bookmark_path.clear();
            self.bookmarks_publish(&ui.global::<Web>());
        }
    }

    fn bookmarks_save(&self, web: &Web<'_>) {
        if let Some(path) = bookmarks::store_path() {
            if let Err(error) = self.browser.bookmarks.save(&path) {
                web.set_bookmark_status(SharedString::from(format!("Cannot save bookmarks: {error}")));
            }
        }
    }

    /// Reads the sources on a worker thread; `bookmarks_poll` applies the result.
    pub(super) fn bookmarks_import(&mut self, ui: &AppWindow) {
        if self.browser.import.is_some() {
            return;
        }
        let web = ui.global::<Web>();
        web.set_bookmark_status("".into());
        web.set_bookmark_importing(true);
        let (sender, receiver) = std::sync::mpsc::channel();
        self.browser.import = Some(receiver);
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let sources = bookmarks::sources();
            let found = sources.iter().find_map(|path| bookmarks::import(path).map(|bar| (path.clone(), bar)));
            if found.is_none() {
                eprintln!("agx-gui: bookmark import found nothing usable in {sources:?}");
            }
            // Keep the progress animation visible long enough to read.
            std::thread::sleep(std::time::Duration::from_millis(900).saturating_sub(started.elapsed()));
            let _ = sender.send(found);
        });
    }

    /// Called from the browser timer: applies a finished import and re-fits the bar after a resize.
    pub(super) fn bookmarks_poll(&mut self, ui: &AppWindow) {
        use std::sync::mpsc::TryRecvError;
        self.bookmarks_resize(ui);
        let Some(receiver) = &self.browser.import else { return };
        let found = match receiver.try_recv() {
            Ok(found) => found,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        self.browser.import = None;
        let web = ui.global::<Web>();
        web.set_bookmark_importing(false);
        match found {
            Some((path, bar)) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                web.set_bookmark_status(format!("Imported {} bookmarks from {name}", bar.count()).into());
                self.browser.bookmarks = bar;
                self.browser.icons = Icons::new();
                self.bookmarks_save(&web);
                self.bookmarks_relayout(ui);
            }
            None => web.set_bookmark_status(
                "No bookmarks found. Export from Chrome (Bookmark manager ▸ ⋮ ▸ Export bookmarks) into Downloads, then import."
                    .into(),
            ),
        }
    }

    pub(super) fn bookmarks_toggle(&mut self, ui: &AppWindow) {
        let web = ui.global::<Web>();
        let (title, uri) = (web.get_title().to_string(), self.browser.uri.clone());
        if uri.is_empty() {
            return;
        }
        self.browser.bookmarks.toggle(&title, &uri);
        self.bookmarks_save(&web);
        self.bookmarks_relayout(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(title: &str) -> Entry {
        Entry { title: title.into(), url: Some("https://www.example.org/x".into()), children: vec![] }
    }

    #[test]
    fn narrow_bars_overflow_into_a_menu_and_all_bookmarks_stays_last() {
        let bar = Bar { items: (0..40).map(|i| link(&format!("Bookmark {i}"))).collect(), ..Bar::default() };
        let wide = layout(&bar, 10_000.0, &mut Icons::default());
        assert_eq!(wide.len(), 41);
        assert_eq!(wide.last().unwrap().title, "All Bookmarks");
        let narrow = layout(&bar, 600.0, &mut Icons::default());
        let overflow = &narrow[narrow.len() - 2];
        assert_eq!(overflow.title, "»");
        assert_eq!(narrow.len() - 2 + overflow.children.len(), 40);
        assert_eq!(narrow.last().unwrap().title, "All Bookmarks");
    }

    #[test]
    fn untitled_links_show_their_site_initial() {
        let entry = link("");
        assert_eq!((label(&entry).as_str(), letter(&entry, false).as_str()), ("example.org", "E"));
        assert_eq!(letter(&entry, true), "");
    }

    #[test]
    #[ignore]
    fn real_profile() {
        let bar = bookmarks::import(std::path::Path::new(&std::env::var("BM").unwrap())).unwrap();
        eprintln!("bar {} other {} mobile {} links {}", bar.items.len(), bar.other.len(), bar.mobile.len(), bar.count());
        let d = layout(&bar, 1900.0, &mut Icons::default());
        eprintln!("{:?}", d.iter().map(|e| e.title.clone()).collect::<Vec<_>>());
    }
}
