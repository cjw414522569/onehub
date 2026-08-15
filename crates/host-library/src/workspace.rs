//! Desktop window, multi-tab, split-pane, and focus model (T106).
//!
//! A [`Workspace`] owns multiple [`WindowModel`]s, each with tabs and
//! splittable panes. A single global focus location (window -> tab -> pane)
//! is kept consistent by every operation, tabs can be dragged between
//! windows, and the whole layout serializes to a versioned snapshot that
//! restores deterministically (multi-window state stays consistent). A
//! [`ShortcutMap`] resolves keyboard shortcuts to the same operations.

/// Split direction for a tab's panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// Side-by-side panes.
    Horizontal,
    /// Stacked panes.
    Vertical,
}

/// A pane inside a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneModel {
    /// Stable id.
    pub id: u64,
    /// Display title.
    pub title: String,
}

/// A tab inside a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabModel {
    /// Stable id.
    pub id: u64,
    /// Display title.
    pub title: String,
    /// Panes; one when unsplit.
    pub panes: Vec<PaneModel>,
    /// Split direction when there are multiple panes.
    pub split: Option<SplitDirection>,
    /// Active pane index.
    pub active_pane: usize,
}

/// A desktop window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowModel {
    /// Stable id.
    pub id: u64,
    /// Display title.
    pub title: String,
    /// Tabs in order.
    pub tabs: Vec<TabModel>,
    /// Active tab index.
    pub active_tab: usize,
    /// Window bounds (x, y, width, height).
    pub bounds: (i32, i32, u32, u32),
}

/// The single focused location in the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusLocation {
    /// Window index.
    pub window: usize,
    /// Tab index.
    pub tab: usize,
    /// Pane index.
    pub pane: usize,
}

/// A versioned, deterministic snapshot of the whole workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    /// Snapshot format version.
    pub version: u32,
    /// Windows in order.
    pub windows: Vec<WindowSnapshot>,
    /// Active window index.
    pub active_window: usize,
}

/// A window in a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    /// Window id.
    pub id: u64,
    /// Title.
    pub title: String,
    /// Tabs in order.
    pub tabs: Vec<TabSnapshot>,
    /// Active tab index.
    pub active_tab: usize,
    /// Bounds.
    pub bounds: (i32, i32, u32, u32),
}

/// A tab in a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSnapshot {
    /// Tab id.
    pub id: u64,
    /// Title.
    pub title: String,
    /// Panes in order.
    pub panes: Vec<PaneSnapshot>,
    /// Split direction.
    pub split: Option<SplitDirection>,
    /// Active pane index.
    pub active_pane: usize,
}

/// A pane in a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot {
    /// Pane id.
    pub id: u64,
    /// Title.
    pub title: String,
}

/// Why restoring a snapshot failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError {
    /// The snapshot has no windows.
    NoWindows,
    /// An index (window/tab/pane) is out of range.
    InvalidState,
}

/// A keyboard shortcut -> action binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    /// New tab.
    NewTab,
    /// Close the active tab.
    CloseTab,
    /// Next tab.
    NextTab,
    /// Previous tab.
    PrevTab,
    /// Split panes vertically.
    SplitVertical,
    /// Split panes horizontally.
    SplitHorizontal,
    /// Focus the next pane/tab/window.
    FocusNext,
    /// Focus the previous pane/tab/window.
    FocusPrev,
    /// Move the active tab to the next window (drag-drop equivalent).
    MoveTabToNextWindow,
}

/// A keyboard shortcut map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutMap {
    bindings: Vec<(&'static str, ShortcutAction)>,
}

impl ShortcutMap {
    /// The default shortcut map (Ctrl+... on Windows).
    pub fn default_map() -> Self {
        Self {
            bindings: vec![
                ("ctrl+t", ShortcutAction::NewTab),
                ("ctrl+w", ShortcutAction::CloseTab),
                ("ctrl+tab", ShortcutAction::NextTab),
                ("ctrl+shift+tab", ShortcutAction::PrevTab),
                ("ctrl+shift+[", ShortcutAction::PrevTab),
                ("ctrl+shift+]", ShortcutAction::NextTab),
                ("ctrl+alt+v", ShortcutAction::SplitVertical),
                ("ctrl+alt+h", ShortcutAction::SplitHorizontal),
                ("ctrl+shift+n", ShortcutAction::FocusNext),
                ("ctrl+shift+p", ShortcutAction::FocusPrev),
                ("ctrl+shift+m", ShortcutAction::MoveTabToNextWindow),
            ],
        }
    }

    /// Resolves a key chord to an action.
    pub fn action(&self, chord: &str) -> Option<ShortcutAction> {
        self.bindings
            .iter()
            .find(|(key, _)| *key == chord)
            .map(|(_, action)| *action)
    }
}

/// The desktop workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    windows: Vec<WindowModel>,
    active_window: usize,
    next_id: u64,
}

impl Workspace {
    /// A workspace with one window containing one tab with one pane.
    pub fn new() -> Self {
        let mut workspace = Self {
            windows: Vec::new(),
            active_window: 0,
            next_id: 1,
        };
        workspace.add_window();
        workspace
    }

    fn next(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Adds a window and returns its id.
    pub fn add_window(&mut self) -> u64 {
        let id = self.next();
        let pane_id = self.next();
        let tab_id = self.next();
        self.windows.push(WindowModel {
            id,
            title: format!("Window {}", self.windows.len() + 1),
            tabs: vec![TabModel {
                id: tab_id,
                title: "Terminal".to_owned(),
                panes: vec![PaneModel {
                    id: pane_id,
                    title: "Terminal".to_owned(),
                }],
                split: None,
                active_pane: 0,
            }],
            active_tab: 0,
            bounds: (100, 100, 960, 640),
        });
        self.active_window = self.windows.len() - 1;
        id
    }

    /// The number of windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// The total number of tabs.
    pub fn tab_count(&self) -> usize {
        self.windows.iter().map(|window| window.tabs.len()).sum()
    }

    /// The active window index.
    pub fn active_window_index(&self) -> usize {
        self.active_window
    }

    /// Reads a window.
    pub fn window(&self, index: usize) -> Option<&WindowModel> {
        self.windows.get(index)
    }

    /// The single focused location (window -> tab -> pane).
    pub fn focused(&self) -> Option<FocusLocation> {
        let window = self.active_window;
        let tab = self.windows.get(window)?.active_tab;
        let pane = self.windows.get(window)?.tabs.get(tab)?.active_pane;
        Some(FocusLocation { window, tab, pane })
    }

    /// Adds a tab to a window; returns the tab id.
    pub fn add_tab(&mut self, window_index: usize) -> Option<u64> {
        let id = self.next();
        let pane_id = self.next();
        let window = self.windows.get_mut(window_index)?;
        window.tabs.push(TabModel {
            id,
            title: format!("Terminal {}", window.tabs.len() + 1),
            panes: vec![PaneModel {
                id: pane_id,
                title: "Terminal".to_owned(),
            }],
            split: None,
            active_pane: 0,
        });
        window.active_tab = window.tabs.len() - 1;
        Some(id)
    }

    /// Closes a tab; focus moves to a valid tab.
    pub fn close_tab(&mut self, window_index: usize, tab_index: usize) -> bool {
        let empty = {
            let window = match self.windows.get_mut(window_index) {
                Some(window) => window,
                None => return false,
            };
            if tab_index >= window.tabs.len() {
                return false;
            }
            window.tabs.remove(tab_index);
            window.active_tab = window.active_tab.min(window.tabs.len().saturating_sub(1));
            window.tabs.is_empty()
        };
        if empty {
            // Recreate a fresh tab so the window always has content.
            self.add_tab(window_index);
        }
        true
    }

    /// Splits the active pane; returns the new pane id.
    pub fn split_active_pane(
        &mut self,
        window_index: usize,
        direction: SplitDirection,
    ) -> Option<u64> {
        let pane_id = self.next();
        let window = self.windows.get_mut(window_index)?;
        let active_tab = window.active_tab;
        let tab = window.tabs.get_mut(active_tab)?;
        tab.panes.push(PaneModel {
            id: pane_id,
            title: format!("Pane {}", tab.panes.len() + 1),
        });
        tab.split = Some(direction);
        tab.active_pane = tab.panes.len() - 1;
        Some(pane_id)
    }

    /// Moves a tab from one window to another (drag-drop / shortcut).
    pub fn move_tab(
        &mut self,
        from_window: usize,
        tab_index: usize,
        to_window: usize,
        position: usize,
    ) -> bool {
        if from_window == to_window {
            return false;
        }
        if from_window >= self.windows.len() || to_window >= self.windows.len() {
            return false;
        }
        let tab = {
            let window = &mut self.windows[from_window];
            if tab_index >= window.tabs.len() {
                return false;
            }
            window.tabs.remove(tab_index)
        };
        {
            let target = &mut self.windows[to_window];
            let position = position.min(target.tabs.len());
            target.tabs.insert(position, tab);
            target.active_tab = position;
        }
        // Focus follows the dragged tab.
        self.active_window = to_window;
        // Keep the source window valid (always at least one tab).
        if self.windows[from_window].tabs.is_empty() {
            self.add_tab(from_window);
        } else {
            self.windows[from_window].active_tab = self.windows[from_window]
                .active_tab
                .min(self.windows[from_window].tabs.len() - 1);
        }
        true
    }

    /// Moves the active tab to the next window (wrap-around).
    pub fn move_active_tab_to_next_window(&mut self) -> bool {
        if self.window_count() < 2 {
            return false;
        }
        let from = self.active_window;
        let to = (from + 1) % self.windows.len();
        let tab_index = self.windows[from].active_tab;
        self.move_tab(from, tab_index, to, usize::MAX)
    }

    /// Focuses the next pane (then tab, then window), wrapping around in a
    /// focus ring.
    pub fn focus_next(&mut self) {
        let Some(location) = self.focused() else {
            return;
        };
        let panes = self.windows[location.window].tabs[location.tab].panes.len();
        if location.pane + 1 < panes {
            self.windows[location.window].tabs[location.tab].active_pane += 1;
            return;
        }
        let tabs = self.windows[location.window].tabs.len();
        if location.tab + 1 < tabs {
            self.windows[location.window].active_tab += 1;
            self.windows[location.window].tabs[location.tab + 1].active_pane = 0;
            return;
        }
        let windows = self.windows.len();
        let next_window = (location.window + 1) % windows;
        self.active_window = next_window;
        self.windows[next_window].active_tab = 0;
        if let Some(first) = self.windows[next_window].tabs.first_mut() {
            first.active_pane = 0;
        }
    }

    /// Focuses the previous pane (then tab, then window), wrapping around in
    /// a focus ring.
    pub fn focus_prev(&mut self) {
        let Some(location) = self.focused() else {
            return;
        };
        if location.pane > 0 {
            self.windows[location.window].tabs[location.tab].active_pane -= 1;
            return;
        }
        if location.tab > 0 {
            self.windows[location.window].active_tab -= 1;
            let previous_tab = location.tab - 1;
            let panes = self.windows[location.window].tabs[previous_tab].panes.len();
            self.windows[location.window].tabs[previous_tab].active_pane = panes - 1;
            return;
        }
        let windows = self.windows.len();
        let previous_window = (location.window + windows - 1) % windows;
        self.active_window = previous_window;
        let last_tab = self.windows[previous_window].tabs.len() - 1;
        self.windows[previous_window].active_tab = last_tab;
        let panes = self.windows[previous_window].tabs[last_tab].panes.len();
        self.windows[previous_window].tabs[last_tab].active_pane = panes - 1;
    }

    /// Serializes the workspace to a versioned snapshot.
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            version: 1,
            windows: self
                .windows
                .iter()
                .map(|window| WindowSnapshot {
                    id: window.id,
                    title: window.title.clone(),
                    tabs: window
                        .tabs
                        .iter()
                        .map(|tab| TabSnapshot {
                            id: tab.id,
                            title: tab.title.clone(),
                            panes: tab
                                .panes
                                .iter()
                                .map(|pane| PaneSnapshot {
                                    id: pane.id,
                                    title: pane.title.clone(),
                                })
                                .collect(),
                            split: tab.split,
                            active_pane: tab.active_pane,
                        })
                        .collect(),
                    active_tab: window.active_tab,
                    bounds: window.bounds,
                })
                .collect(),
            active_window: self.active_window,
        }
    }

    /// Restores a workspace from a snapshot (validates all indices).
    pub fn restore(snapshot: &WorkspaceSnapshot) -> Result<Self, RestoreError> {
        if snapshot.windows.is_empty() {
            return Err(RestoreError::NoWindows);
        }
        if snapshot.active_window >= snapshot.windows.len() {
            return Err(RestoreError::InvalidState);
        }
        let mut next_id = 1u64;
        for window in &snapshot.windows {
            if window.active_tab >= window.tabs.len() {
                return Err(RestoreError::InvalidState);
            }
            for tab in &window.tabs {
                if tab.panes.is_empty() || tab.active_pane >= tab.panes.len() {
                    return Err(RestoreError::InvalidState);
                }
                for pane in &tab.panes {
                    next_id = next_id.max(pane.id + 1);
                }
                next_id = next_id.max(tab.id + 1);
            }
            next_id = next_id.max(window.id + 1);
        }
        let windows = snapshot
            .windows
            .iter()
            .map(|window| WindowModel {
                id: window.id,
                title: window.title.clone(),
                tabs: window
                    .tabs
                    .iter()
                    .map(|tab| TabModel {
                        id: tab.id,
                        title: tab.title.clone(),
                        panes: tab
                            .panes
                            .iter()
                            .map(|pane| PaneModel {
                                id: pane.id,
                                title: pane.title.clone(),
                            })
                            .collect(),
                        split: tab.split,
                        active_pane: tab.active_pane,
                    })
                    .collect(),
                active_tab: window.active_tab,
                bounds: window.bounds,
            })
            .collect();
        Ok(Self {
            windows,
            active_window: snapshot.active_window,
            next_id,
        })
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{RestoreError, ShortcutAction, ShortcutMap, SplitDirection, Workspace};

    #[test]
    fn new_workspace_has_one_window_one_tab_and_consistent_focus() {
        let workspace = Workspace::new();
        assert_eq!(workspace.window_count(), 1);
        assert_eq!(workspace.tab_count(), 1);
        assert_eq!(
            workspace.focused(),
            Some(super::FocusLocation {
                window: 0,
                tab: 0,
                pane: 0
            })
        );
    }

    #[test]
    fn tabs_and_splits_track_active_pane() {
        let mut workspace = Workspace::new();
        workspace.add_tab(0).unwrap();
        assert_eq!(workspace.tab_count(), 2);
        let pane = workspace
            .split_active_pane(0, SplitDirection::Horizontal)
            .unwrap();
        assert!(pane > 0);
        assert_eq!(workspace.focused().unwrap().pane, 1);
        // Split again: three panes, still focused on the newest.
        workspace
            .split_active_pane(0, SplitDirection::Vertical)
            .unwrap();
        assert_eq!(workspace.focused().unwrap().pane, 2);
    }

    #[test]
    fn drag_drop_moves_tab_between_windows() {
        let mut workspace = Workspace::new();
        workspace.add_window();
        workspace.add_tab(0).unwrap();
        assert_eq!(workspace.window(0).unwrap().tabs.len(), 2);
        assert_eq!(workspace.window(1).unwrap().tabs.len(), 1);
        // Drag tab 0 from window 0 into window 1 at the end.
        assert!(workspace.move_tab(0, 0, 1, usize::MAX));
        assert_eq!(workspace.window(0).unwrap().tabs.len(), 1);
        assert_eq!(workspace.window(1).unwrap().tabs.len(), 2);
        assert_eq!(workspace.active_window_index(), 1, "focus follows the drag");
    }

    #[test]
    fn close_tab_keeps_focus_consistent() {
        let mut workspace = Workspace::new();
        workspace.add_tab(0).unwrap();
        workspace.add_tab(0).unwrap(); // tabs [T0, T1, T2], active 2
        workspace.focus_next(); // focus ring wraps to tab 0
        assert_eq!(workspace.focused().unwrap().tab, 0);
        assert!(workspace.close_tab(0, 1)); // remove T1 -> [T0, T2]
        assert_eq!(workspace.focused().unwrap().tab, 0);
        assert!(workspace.close_tab(0, 0)); // remove T0 -> [T2]
        assert_eq!(workspace.focused().unwrap().tab, 0);
        assert_eq!(workspace.tab_count(), 1);
    }

    #[test]
    fn shortcuts_resolve_to_actions() {
        let map = ShortcutMap::default_map();
        assert_eq!(map.action("ctrl+t"), Some(ShortcutAction::NewTab));
        assert_eq!(map.action("ctrl+w"), Some(ShortcutAction::CloseTab));
        assert_eq!(map.action("ctrl+shift+["), Some(ShortcutAction::PrevTab));
        assert_eq!(map.action("alt+f4"), None);
    }

    #[test]
    fn focus_next_prev_cycles_panes_tabs_windows() {
        let mut workspace = Workspace::new();
        workspace.add_window(); // active window is now 1
        workspace.add_tab(0).unwrap();
        workspace
            .split_active_pane(0, SplitDirection::Horizontal)
            .unwrap();
        // Focus ring starting at window 1, tab 0, pane 0.
        workspace.focus_next(); // wraps to window 0, tab 0, pane 0
        assert_eq!(
            workspace.focused(),
            Some(super::FocusLocation {
                window: 0,
                tab: 0,
                pane: 0
            })
        );
        workspace.focus_next(); // window 0, tab 1, pane 0
        assert_eq!(
            workspace.focused(),
            Some(super::FocusLocation {
                window: 0,
                tab: 1,
                pane: 0
            })
        );
        workspace.focus_next(); // window 0, tab 1, pane 1 (split pane)
        assert_eq!(
            workspace.focused(),
            Some(super::FocusLocation {
                window: 0,
                tab: 1,
                pane: 1
            })
        );
        workspace.focus_prev(); // window 0, tab 1, pane 0
        assert_eq!(
            workspace.focused(),
            Some(super::FocusLocation {
                window: 0,
                tab: 1,
                pane: 0
            })
        );
        workspace.focus_prev(); // window 0, tab 0, pane 0
        assert_eq!(
            workspace.focused(),
            Some(super::FocusLocation {
                window: 0,
                tab: 0,
                pane: 0
            })
        );
        workspace.focus_prev(); // wraps to window 1, tab 0, pane 0
        assert_eq!(
            workspace.focused(),
            Some(super::FocusLocation {
                window: 1,
                tab: 0,
                pane: 0
            })
        );
    }

    #[test]
    fn snapshot_restore_round_trip_preserves_multi_window_state() {
        let mut workspace = Workspace::new();
        workspace.add_window();
        workspace.add_tab(0).unwrap();
        workspace
            .split_active_pane(0, SplitDirection::Horizontal)
            .unwrap();
        workspace.add_tab(1).unwrap();
        let snapshot = workspace.snapshot();
        let restored = Workspace::restore(&snapshot).unwrap();
        assert_eq!(restored.snapshot(), snapshot, "restore is a fixed point");
        assert_eq!(restored.window_count(), 2);
        assert_eq!(restored.tab_count(), 4);
        assert_eq!(restored.focused(), workspace.focused());
        // Invalid snapshots are rejected.
        let mut bad = snapshot.clone();
        bad.active_window = 99;
        assert_eq!(Workspace::restore(&bad), Err(RestoreError::InvalidState));
        let mut empty = snapshot.clone();
        empty.windows.clear();
        assert_eq!(Workspace::restore(&empty), Err(RestoreError::NoWindows));
    }
}
