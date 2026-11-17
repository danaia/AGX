//! App catalogue, searchable desktop commands and launcher model updates.
use crate::{
    App, AppWindow, MenuRow,
    keymap::Cmd,
    wm::{Desktop, Kind, Mode, WORKSPACES},
};
use slint::{ModelRc, VecModel};

struct Entry {
    label: String,
    description: String,
    hint: String,
    aliases: &'static str,
    icon: i32,
    app: bool,
    running: bool,
    command: Cmd,
}

fn catalogue(desktop: &Desktop, modifier: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    for (kind, (label, description, aliases)) in Kind::ALL.into_iter().zip([
        (
            "Overview",
            "Your Pi at a glance",
            "angi dashboard home status",
        ),
        (
            "CPU Monitor",
            "Processor activity, load, and temperature",
            "performance processor usage cpu",
        ),
        (
            "Memory & Storage",
            "Memory usage and available disk space",
            "ram disk drive storage memory",
        ),
        (
            "Network",
            "Connections, addresses, and remote access",
            "wifi wi-fi ethernet internet vnc ssh",
        ),
        (
            "Hardware",
            "Explore devices connected to your Pi",
            "devices board usb peripherals",
        ),
        (
            "Services",
            "See what is running on your system",
            "process daemon background service",
        ),
        (
            "System",
            "Operating system and machine details",
            "settings info version kernel host",
        ),
    ]) {
        entries.push(Entry {
            label: label.into(),
            description: description.into(),
            hint: if desktop.is_open(kind) { "Running" } else { "" }.into(),
            aliases,
            icon: kind.index() as i32,
            app: true,
            running: desktop.is_open(kind),
            command: Cmd::Open(kind),
        });
    }
    entries.push(Entry {
        label: "Terminal Console".into(),
        description: "Switch to ANGI’s text-based tools".into(),
        hint: "TUI".into(),
        aliases: "terminal console shell tui",
        icon: 7,
        app: true,
        running: false,
        command: Cmd::Tui,
    });
    let mut command = |label: String, description: String, hint: String, icon, cmd| {
        entries.push(Entry {
            label,
            description,
            hint,
            icon,
            command: cmd,
            aliases: "",
            app: false,
            running: false,
        });
    };
    if let Some(focused) = desktop.focused() {
        let title = focused.title();
        for (label, description, shortcut, cmd) in [
            (
                "Close window",
                format!("Close {title}; reopen it from Apps"),
                "Q",
                Cmd::Close,
            ),
            (
                "Float or tile window",
                format!("Change how {title} is arranged"),
                "T",
                Cmd::ToggleFloat,
            ),
            (
                "Fullscreen window",
                format!("Give {title} the whole screen"),
                "F",
                Cmd::Fullscreen,
            ),
            (
                "Maximize window",
                format!("Expand {title} within the desktop"),
                "",
                Cmd::Maximize,
            ),
            (
                "Toggle split",
                "Change the direction of tiled windows".into(),
                "J",
                Cmd::ToggleSplit,
            ),
        ] {
            command(
                label.into(),
                description,
                if shortcut.is_empty() {
                    "".into()
                } else {
                    format!("{modifier}+{shortcut}")
                },
                9,
                cmd,
            );
        }
    }
    command(
        if desktop.mode() == Mode::Tiled {
            "Windowed mode"
        } else {
            "Tiled mode"
        }
        .into(),
        "Change the layout of this workspace".into(),
        format!("{modifier}+L"),
        9,
        Cmd::CycleLayout,
    );
    for n in 0..WORKSPACES {
        command(
            format!("Workspace {}", n + 1),
            "Switch desktop workspace".into(),
            format!("{modifier}+{}", n + 1),
            8,
            Cmd::Workspace(n),
        );
    }
    command(
        "Sign out".into(),
        "Return to the sign-in screen".into(),
        "".into(),
        11,
        Cmd::Logout,
    );
    command(
        "Restart".into(),
        "Reboot your Pi after confirmation".into(),
        "".into(),
        10,
        Cmd::Reboot,
    );
    command(
        "Shut down".into(),
        "Power off your Pi after confirmation".into(),
        "".into(),
        10,
        Cmd::Shutdown,
    );
    entries
}

fn results(desktop: &Desktop, modifier: &str, query: &str, section: i32) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    let mut entries: Vec<_> = catalogue(desktop, modifier)
        .into_iter()
        .filter(|entry| {
            if query.is_empty() {
                return entry.app == (section == 0);
            }
            let searchable =
                format!("{} {} {}", entry.label, entry.description, entry.aliases).to_lowercase();
            query
                .split_whitespace()
                .all(|word| searchable.contains(word))
        })
        .collect();
    if !query.is_empty() {
        // A title match wins over an incidental word in a description.
        entries.sort_by_key(|entry| (!entry.label.to_lowercase().starts_with(&query), !entry.app));
    }
    entries
}

impl App {
    pub(super) fn set_menu_rows(&mut self, ui: &AppWindow, query: &str) {
        let entries = results(&self.desk, self.modk.name(), query, ui.get_menu_section());
        let rows: Vec<_> = entries
            .iter()
            .map(|entry| MenuRow {
                label: entry.label.clone().into(),
                description: entry.description.clone().into(),
                hint: entry.hint.clone().into(),
                icon: entry.icon,
                app: entry.app,
                running: entry.running,
            })
            .collect();
        self.menu = entries.into_iter().map(|entry| entry.command).collect();
        ui.set_menu_rows(ModelRc::new(VecModel::from(rows)));
    }

    pub(super) fn open_menu(&mut self, ui: &AppWindow) {
        ui.set_session_open(false);
        ui.set_menu_section(0);
        self.set_menu_rows(ui, "");
        ui.set_menu_index(0);
        ui.set_menu_open(true);
    }

    pub(super) fn menu_accept(&mut self, ui: &AppWindow, index: i32) {
        if let Some(command) = self.menu.get(index as usize).copied() {
            self.run(ui, command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wm::Rect;

    fn desktop() -> Desktop {
        Desktop::new(
            Rect::new(0., 0., 1280., 720.),
            Rect::new(0., 34., 1280., 660.),
        )
    }

    #[test]
    fn apps_include_running_and_closed_tools_and_open_them_from_any_workspace() {
        let mut desktop = desktop();
        desktop.close(Kind::Services);
        let apps = results(&desktop, "Super", "", 0);
        assert_eq!(apps.len(), 8);
        assert!(apps.iter().all(|entry| entry.app));
        assert_eq!(apps[0].command, Cmd::Open(Kind::Overview));
        assert!(
            apps.iter()
                .any(|entry| entry.command == Cmd::Open(Kind::Services) && !entry.running)
        );
        desktop.goto(4);
        desktop.open(Kind::Overview);
        assert_eq!(desktop.focused(), Some(Kind::Overview));
    }

    #[test]
    fn search_finds_app_aliases_and_secondary_commands() {
        let desktop = desktop();
        for query in ["wifi", "  WIFI  ", "remote access"] {
            assert_eq!(
                results(&desktop, "Super", query, 0)[0].command,
                Cmd::Open(Kind::Network)
            );
        }
        assert_eq!(
            results(&desktop, "Super", "ram", 0)[0].command,
            Cmd::Open(Kind::Memory)
        );
        assert_eq!(
            results(&desktop, "Super", "workspace 3", 0)[0].command,
            Cmd::Workspace(2)
        );
        assert!(results(&desktop, "Super", "not-an-app", 0).is_empty());
        assert!(
            results(&desktop, "Super", "", 1)
                .iter()
                .all(|entry| !entry.app)
        );
    }
}
