//! Installable packages in the launcher: how each state looks, and what choosing a row does.
//!
//! Choosing an app that is not installed installs it in place; the launcher stays open, the row shows
//! live progress, and the program is on the terminal's PATH the moment it finishes. Choosing an installed
//! app opens it in a terminal; choosing one with an update waiting installs the update.
use super::{APP_ICON, APPS, Entry, Status, apps, apps::Package};
use crate::{App, AppWindow, keymap::Cmd, library::Availability};

/// `MenuRow.state` values for package rows; 0 is every other row.
const AVAILABLE: i32 = 1;
const QUEUED: i32 = 2;
const WORKING: i32 = 3;
const INSTALLED: i32 = 4;
const UPDATE: i32 = 5;
const FAILED: i32 = 6;

pub(super) fn package(index: usize) -> Option<&'static Package> {
    apps::index().get(index)
}

pub(super) fn entry(index: usize, package: &'static Package, availability: Availability) -> Entry {
    let summary = package.summary.clone();
    let (state, hint, description, progress, action, keeps_open, removable, running) = match availability {
        Availability::Available => (AVAILABLE, package.kind.clone(), summary, 0.0, "Install", true, false, false),
        Availability::Queued { removing } => (
            QUEUED,
            if removing { "Removing…" } else { "Waiting…" }.into(),
            summary,
            0.0,
            "",
            true,
            false,
            false,
        ),
        Availability::Working { fraction, message, removing } => (
            WORKING,
            if removing { "Removing…".into() } else { format!("{}%", (fraction * 100.0).round() as u32) },
            message,
            fraction,
            "",
            true,
            false,
            false,
        ),
        Availability::Failed(error) => (FAILED, "Retry".into(), error, 0.0, "Retry", true, false, false),
        Availability::Installed { tag, update: None } => {
            (INSTALLED, format!("Installed · {tag}"), summary, 0.0, "Open", false, true, true)
        }
        Availability::Installed { tag, update: Some(latest) } => (
            UPDATE,
            "Update available".into(),
            format!("{tag} → {latest}  ·  {summary}"),
            0.0,
            "Update",
            true,
            true,
            true,
        ),
    };
    Entry {
        label: package.name.clone(),
        description,
        hint,
        aliases: &package.aliases,
        icon: APP_ICON,
        section: APPS,
        running,
        command: Cmd::Package(index),
        status: Status { state, progress, action: action.into(), keeps_open, removable },
        badge: if package.recommended { "Recommended" } else { "" },
        heading: String::new(),
    }
}

/// An app that is installed (up to date or with an update waiting).
pub(super) fn is_installed(entry: &Entry) -> bool {
    matches!(entry.status.state, INSTALLED | UPDATE)
}

/// The Apps tab's own layout: what is installed first, then everything else in catalogue order, each
/// group introduced by a heading on its first row. An app that is installing stays where it is until it
/// finishes, so a row never moves under the pointer mid-install.
pub(super) fn arrange(entries: Vec<Entry>) -> Vec<Entry> {
    let (mut installed, available): (Vec<_>, Vec<_>) = entries.into_iter().partition(is_installed);
    let (count, rest) = (installed.len(), available.len());
    let mut all: Vec<Entry> = Vec::with_capacity(count + rest);
    if let Some(first) = installed.first_mut() {
        first.heading = format!("INSTALLED  ·  {count}");
    }
    all.append(&mut installed);
    all.extend(available);
    if let Some(first) = all.get_mut(count) {
        first.heading = format!("{}  ·  {rest}", if count > 0 { "AVAILABLE" } else { "ALL APPS" });
    }
    all
}

impl App {
    /// Re-renders the open launcher with the current query. The selection follows the same entry even when
    /// it moves, as an app does from Available to Installed when its install finishes.
    pub(crate) fn refresh_menu(&mut self, ui: &AppWindow) {
        let selected = usize::try_from(ui.get_menu_index()).ok().and_then(|i| self.menu.get(i)).copied();
        let query = self.menu_query.clone();
        self.set_menu_rows(ui, &query);
        if let Some(index) = selected.and_then(|cmd| self.menu.iter().position(|other| *other == cmd)) {
            if index as i32 != ui.get_menu_index() {
                ui.set_menu_index(index as i32);
            }
        }
    }

    pub(crate) fn package_activate(&mut self, ui: &AppWindow, index: usize) {
        let Some(package) = package(index) else {
            return;
        };
        match self.library.availability(&package.id) {
            Availability::Installed { update: None, .. } => self.package_open(ui, package),
            Availability::Queued { .. } | Availability::Working { .. } => {}
            Availability::Available | Availability::Failed(_) | Availability::Installed { .. } => {
                self.library.install(&package.id);
                self.refresh_menu(ui);
            }
        }
    }

    /// Opens a terminal for an installed app: the program itself when it is a full-screen tool, else a shell.
    fn package_open(&mut self, ui: &AppWindow, package: &Package) {
        if package.id == "chromium" {
            self.desk.goto(self.menu_workspace);
            self.chromium_open(ui, Vec::new());
            return;
        }
        if package.kind == "Graphical" && package.launch.is_empty() {
            // A windowed app needs a Wayland display, which ANGI does not provide yet. Say so rather than
            // starting it in a terminal where it would only print an error.
            self.toast(
                ui,
                &format!("{} is installed. It cannot open yet: ANGI's Wayland display is not ready.", package.name),
            );
            return;
        }
        self.desk.goto(self.menu_workspace);
        if package.launch.is_empty() {
            self.run(ui, Cmd::Open(crate::wm::Kind::Terminal));
            let command = package.commands.first().map(String::as_str).unwrap_or(&package.name);
            self.toast(ui, &format!("{} is installed. Type {command} to run it.", package.name));
            return;
        }
        let path = crate::library::bin_directory().join(&package.launch);
        let path = path.to_string_lossy().into_owned();
        self.terminal_open_program(ui, path, package.launch.clone());
    }

    /// Removes the installed app on launcher row `row`. Its settings and data are left alone.
    pub(crate) fn menu_remove(&mut self, ui: &AppWindow, row: i32) {
        if let Some(Cmd::Package(index)) = self.menu.get(row as usize).copied() {
            self.package_remove(ui, index);
        }
    }

    fn package_remove(&mut self, ui: &AppWindow, index: usize) {
        if let Some(package) = package(index) {
            if matches!(self.library.availability(&package.id), Availability::Installed { .. }) {
                self.library.remove(&package.id);
                self.refresh_menu(ui);
            }
        }
    }
}
