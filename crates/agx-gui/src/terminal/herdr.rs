//! Bringing the Herdr window forward for agents that are already running.
//!
//! Closing a terminal tab detaches Herdr but leaves its swarm running, so the desktop offers a way
//! back in. A tab that is already attached is shown rather than duplicated.
use super::Session;
use crate::{App, AppWindow};

impl App {
    pub(crate) fn terminal_open_herdr(&mut self, ui: &AppWindow) {
        // Every terminal window's tabs: the current context, then those parked in the map.
        let attached = std::iter::once((self.terminal_kind, &self.terminal))
            .chain(self.terminal_windows.iter().map(|(kind, state)| (*kind, state)))
            .filter(|(kind, _)| self.desk.is_open(*kind))
            .find_map(|(kind, state)| {
                state.sessions.iter().position(Session::runs_herdr).map(|tab| (kind, tab))
            });
        let Some((kind, tab)) = attached else {
            self.terminal_launch(ui, Session::herdr);
            return;
        };
        self.terminal_select(ui, kind);
        self.terminal.active = tab;
        self.terminal.dirty();
        // Brings the window to this workspace, restores it if collapsed and focuses it.
        self.desk.open(kind);
        self.refresh(ui);
    }
}
