//! Command palette and full keyboard navigation (T120).
//!
//! [`CommandPalette`] filters commands by title / keywords and executes the
//! selected one. [`KeyboardFlow`] drives the palette with keyboard events
//! only (open / type / navigate / enter / escape) and applies the executed
//! actions to the session state, so connect, switch, search, forward, and
//! disconnect are all completable **without a mouse** (verified by the
//! keyboard end-to-end test).

/// A palette action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    /// Connect to a host.
    Connect(u64),
    /// Switch to a tab.
    SwitchTab(u64),
    /// Switch to a window.
    SwitchWindow(u64),
    /// Open host search.
    SearchHosts,
    /// Create a port forward.
    PortForward,
    /// Disconnect from a host.
    Disconnect(u64),
}

/// A command in the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteCommand {
    /// Stable id.
    pub id: u64,
    /// The title.
    pub title: &'static str,
    /// The category.
    pub category: &'static str,
    /// Search keywords.
    pub keywords: &'static [&'static str],
    /// The action.
    pub action: PaletteAction,
}

impl PaletteCommand {
    /// Whether the command matches a query (title or keywords,
    /// case-insensitive).
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        if query.is_empty() {
            return true;
        }
        self.title.to_lowercase().contains(&query)
            || self
                .keywords
                .iter()
                .any(|keyword| keyword.to_lowercase().contains(&query))
    }
}

/// The command palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPalette {
    /// All commands.
    pub commands: Vec<PaletteCommand>,
    /// Whether the palette is open.
    pub open: bool,
    /// The current query.
    pub query: String,
    /// Matching command ids (in command order).
    pub results: Vec<u64>,
    /// The selected result index.
    pub selected: usize,
}

impl CommandPalette {
    /// A palette over the given commands (closed).
    pub fn new(commands: Vec<PaletteCommand>) -> Self {
        let mut palette = Self {
            commands,
            open: false,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
        };
        palette.refresh();
        palette
    }

    fn refresh(&mut self) {
        self.results = self
            .commands
            .iter()
            .filter(|command| command.matches(&self.query))
            .map(|command| command.id)
            .collect();
        if self.selected >= self.results.len() && !self.results.is_empty() {
            self.selected = self.results.len() - 1;
        }
    }

    /// Opens the palette.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.refresh();
    }

    /// Closes the palette.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Toggles the palette.
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// Types a character into the query.
    pub fn type_char(&mut self, character: char) {
        if !self.open {
            return;
        }
        self.query.push(character);
        self.selected = 0;
        self.refresh();
    }

    /// Removes the last query character.
    pub fn backspace(&mut self) {
        if !self.open {
            return;
        }
        self.query.pop();
        self.selected = 0;
        self.refresh();
    }

    /// Selects the next result (wraps).
    pub fn next(&mut self) {
        if self.results.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.results.len();
    }

    /// Selects the previous result (wraps).
    pub fn prev(&mut self) {
        if self.results.is_empty() {
            return;
        }
        self.selected = (self.selected + self.results.len() - 1) % self.results.len();
    }

    /// The currently selected command, if any.
    pub fn selected_command(&self) -> Option<&PaletteCommand> {
        let id = *self.results.get(self.selected)?;
        self.commands.iter().find(|command| command.id == id)
    }

    /// Executes the selected command (closes the palette) and returns its
    /// action.
    pub fn execute_selected(&mut self) -> Option<PaletteAction> {
        let action = self.selected_command().map(|command| command.action);
        self.close();
        action
    }
}

/// A keyboard event for the flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowKey {
    /// Toggle the palette.
    TogglePalette,
    /// Type a character into the query.
    Type(char),
    /// Remove the last character.
    Backspace,
    /// Select the next result.
    Next,
    /// Select the previous result.
    Prev,
    /// Execute the selected command.
    Enter,
    /// Close the palette.
    Escape,
}

/// The keyboard-driven flow over the session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardFlow {
    palette: CommandPalette,
    /// Actions executed so far.
    pub actions: Vec<PaletteAction>,
    /// The connected host.
    pub connected_host: Option<u64>,
    /// The current tab.
    pub current_tab: u64,
    /// The current window.
    pub current_window: u64,
    /// Whether a port forward is active.
    pub forwarded: bool,
    /// Whether host search is open.
    pub searching: bool,
}

impl KeyboardFlow {
    /// A flow over a palette and initial session state.
    pub fn new(palette: CommandPalette, current_tab: u64, current_window: u64) -> Self {
        Self {
            palette,
            actions: Vec::new(),
            connected_host: None,
            current_tab,
            current_window,
            forwarded: false,
            searching: false,
        }
    }

    /// Handles a keyboard event; returns the executed action, if any.
    pub fn handle(&mut self, key: FlowKey) -> Option<PaletteAction> {
        match key {
            FlowKey::TogglePalette => {
                self.palette.toggle();
                None
            }
            FlowKey::Type(character) => {
                self.palette.type_char(character);
                None
            }
            FlowKey::Backspace => {
                self.palette.backspace();
                None
            }
            FlowKey::Next => {
                self.palette.next();
                None
            }
            FlowKey::Prev => {
                self.palette.prev();
                None
            }
            FlowKey::Escape => {
                self.palette.close();
                None
            }
            FlowKey::Enter => {
                if !self.palette.open {
                    return None;
                }
                let action = self.palette.execute_selected()?;
                self.apply(action);
                self.actions.push(action);
                Some(action)
            }
        }
    }

    fn apply(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::Connect(host) => self.connected_host = Some(host),
            PaletteAction::SwitchTab(tab) => self.current_tab = tab,
            PaletteAction::SwitchWindow(window) => self.current_window = window,
            PaletteAction::SearchHosts => self.searching = true,
            PaletteAction::PortForward => self.forwarded = true,
            PaletteAction::Disconnect(host) => {
                if self.connected_host == Some(host) {
                    self.connected_host = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandPalette, FlowKey, KeyboardFlow, PaletteAction, PaletteCommand};

    fn commands() -> Vec<PaletteCommand> {
        vec![
            PaletteCommand {
                id: 1,
                title: "Connect to host",
                category: "session",
                keywords: &["connect", "ssh"],
                action: PaletteAction::Connect(7),
            },
            PaletteCommand {
                id: 2,
                title: "Switch tab",
                category: "window",
                keywords: &["switch", "tab"],
                action: PaletteAction::SwitchTab(2),
            },
            PaletteCommand {
                id: 3,
                title: "Switch window",
                category: "window",
                keywords: &["switch", "window"],
                action: PaletteAction::SwitchWindow(1),
            },
            PaletteCommand {
                id: 4,
                title: "Search hosts",
                category: "navigation",
                keywords: &["search", "hosts"],
                action: PaletteAction::SearchHosts,
            },
            PaletteCommand {
                id: 5,
                title: "Port forward",
                category: "network",
                keywords: &["forward", "port"],
                action: PaletteAction::PortForward,
            },
            PaletteCommand {
                id: 6,
                title: "Disconnect",
                category: "session",
                keywords: &["disconnect", "close"],
                action: PaletteAction::Disconnect(7),
            },
        ]
    }

    #[test]
    fn palette_filters_and_navigates() {
        let mut palette = CommandPalette::new(commands());
        palette.open();
        assert_eq!(palette.results.len(), 6);
        palette.type_char('f');
        // Matches "forward" and "Disconnect"? No: only title/keywords contain
        // "f" -> Port forward (f), Disconnect (none)... assert >= 1.
        assert!(!palette.results.is_empty());
        palette.type_char('o');
        palette.type_char('r');
        palette.type_char('w');
        assert_eq!(
            palette.selected_command().map(|c| c.title),
            Some("Port forward")
        );
        palette.next();
        palette.prev();
        assert_eq!(
            palette.selected_command().map(|c| c.title),
            Some("Port forward")
        );
        assert_eq!(palette.execute_selected(), Some(PaletteAction::PortForward));
        assert!(!palette.open, "executing closes the palette");
    }

    #[test]
    fn keyboard_end_to_end_without_mouse() {
        let palette = CommandPalette::new(commands());
        let mut flow = KeyboardFlow::new(palette, 0, 0);

        // Connect: Ctrl+K, type "conn", Enter.
        flow.handle(FlowKey::TogglePalette);
        for character in "conn".chars() {
            flow.handle(FlowKey::Type(character));
        }
        assert_eq!(flow.handle(FlowKey::Enter), Some(PaletteAction::Connect(7)));
        assert_eq!(flow.connected_host, Some(7));

        // Switch tab.
        flow.handle(FlowKey::TogglePalette);
        for character in "switch".chars() {
            flow.handle(FlowKey::Type(character));
        }
        flow.handle(FlowKey::Enter);
        assert_eq!(flow.current_tab, 2);

        // Search hosts.
        flow.handle(FlowKey::TogglePalette);
        for character in "search".chars() {
            flow.handle(FlowKey::Type(character));
        }
        flow.handle(FlowKey::Enter);
        assert!(flow.searching);

        // Port forward.
        flow.handle(FlowKey::TogglePalette);
        for character in "forward".chars() {
            flow.handle(FlowKey::Type(character));
        }
        flow.handle(FlowKey::Enter);
        assert!(flow.forwarded);

        // Disconnect.
        flow.handle(FlowKey::TogglePalette);
        for character in "disconn".chars() {
            flow.handle(FlowKey::Type(character));
        }
        flow.handle(FlowKey::Enter);
        assert_eq!(flow.connected_host, None);

        // Every action happened through the keyboard; no mouse involved.
        assert_eq!(flow.actions.len(), 5);
        assert_eq!(flow.actions[0], PaletteAction::Connect(7));
    }

    #[test]
    fn escape_closes_and_backspace_edits_query() {
        let mut palette = CommandPalette::new(commands());
        palette.open();
        palette.type_char('s');
        palette.type_char('w');
        assert_eq!(palette.query, "sw");
        palette.backspace();
        assert_eq!(palette.query, "s");
        let mut flow = KeyboardFlow::new(CommandPalette::new(commands()), 0, 0);
        flow.handle(FlowKey::TogglePalette);
        assert!(flow.palette.open);
        flow.handle(FlowKey::Escape);
        assert!(!flow.palette.open);
        // Enter with the palette closed does nothing.
        assert_eq!(flow.handle(FlowKey::Enter), None);
    }
}
