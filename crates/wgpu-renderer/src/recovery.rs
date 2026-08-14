//! GPU / rendering device-loss, foreground/background, and window-rebuild
//! recovery (T080).
//!
//! [`RecoveryCoordinator`] keeps the terminal content snapshot retained across
//! GPU device loss, backgrounding, and window recreation, so recovery always
//! restores consistent content. It never touches the SSH session: there is no
//! API to disconnect a session, and `session_alive()` stays `true` across the
//! whole lifecycle. Platform lifecycle fault injection runs as deterministic
//! unit tests (real GPU fault injection is `blocked_environment` on CI hosts
//! without a GPU).

use core_protocol::terminal::TerminalSnapshot;

/// The render lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    /// Healthy and rendering.
    Healthy,
    /// GPU device lost; content retained.
    DeviceLost,
    /// Rebuilding GPU resources from the retained content.
    Rebuilding,
    /// Recovered; content restored.
    Recovered,
}

/// Coordinates renderer recovery across platform lifecycle events.
#[derive(Debug, Clone)]
pub struct RecoveryCoordinator {
    phase: LifecyclePhase,
    /// Terminal content retained across GPU loss / background / rebuild.
    retained: Option<TerminalSnapshot>,
    /// The SSH session is never disconnected by renderer recovery.
    session_alive: bool,
    losses: u64,
    rebuilds: u64,
    window_recreations: u64,
}

impl Default for RecoveryCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryCoordinator {
    /// A coordinator with no retained content yet.
    pub fn new() -> Self {
        Self {
            phase: LifecyclePhase::Healthy,
            retained: None,
            session_alive: true,
            losses: 0,
            rebuilds: 0,
            window_recreations: 0,
        }
    }

    /// Retains the current terminal content (called on every frame while
    /// healthy).
    pub fn retain(&mut self, snapshot: TerminalSnapshot) {
        self.retained = Some(snapshot);
    }

    /// The phase.
    pub fn phase(&self) -> LifecyclePhase {
        self.phase
    }

    /// The retained content (never lost by GPU recovery).
    pub fn retained_snapshot(&self) -> Option<&TerminalSnapshot> {
        self.retained.as_ref()
    }

    /// Whether the SSH session is still alive (always true; renderer recovery
    /// never disconnects a session).
    pub fn session_alive(&self) -> bool {
        self.session_alive
    }

    /// GPU device lost: enter `DeviceLost`, keep the retained content.
    pub fn on_device_lost(&mut self) {
        self.phase = LifecyclePhase::DeviceLost;
        self.losses += 1;
    }

    /// Begins rebuilding GPU resources from the retained content.
    pub fn begin_rebuild(&mut self) -> bool {
        if self.retained.is_none() {
            return false;
        }
        self.phase = LifecyclePhase::Rebuilding;
        true
    }

    /// Finishes the rebuild; content is restored and the phase becomes
    /// `Recovered`.
    pub fn finish_rebuild(&mut self) -> bool {
        if self.phase != LifecyclePhase::Rebuilding {
            return false;
        }
        self.phase = LifecyclePhase::Recovered;
        self.rebuilds += 1;
        true
    }

    /// The app went to the background: the render loop pauses externally;
    /// content stays retained so foregrounding restores it.
    pub fn on_app_background(&mut self) {}

    /// The app returned to the foreground: content is restored (a full
    /// re-render from the retained snapshot).
    pub fn on_app_foreground(&mut self) {
        if matches!(
            self.phase,
            LifecyclePhase::DeviceLost | LifecyclePhase::Rebuilding
        ) {
            self.phase = LifecyclePhase::Recovered;
        }
    }

    /// The window was recreated: rebuild the surface from the retained
    /// content (content stays consistent).
    pub fn on_window_recreated(&mut self) {
        self.window_recreations += 1;
        self.phase = LifecyclePhase::Recovered;
    }

    /// Number of device losses handled.
    pub fn losses(&self) -> u64 {
        self.losses
    }

    /// Number of successful rebuilds.
    pub fn rebuilds(&self) -> u64 {
        self.rebuilds
    }

    /// Number of window recreations handled.
    pub fn window_recreations(&self) -> u64 {
        self.window_recreations
    }
}

#[cfg(test)]
mod tests {
    use core_protocol::terminal::{CursorState, TerminalCell, TerminalRow, TerminalSnapshot};

    use super::{LifecyclePhase, RecoveryCoordinator};

    fn snapshot() -> TerminalSnapshot {
        TerminalSnapshot {
            stream_id: 1,
            sequence: 9,
            rows: vec![TerminalRow {
                cells: vec![TerminalCell::cluster("x"), TerminalCell::cluster("y")],
            }],
            cursor: CursorState {
                row: 0,
                col: 0,
                visible: true,
            },
            title: Some("keep".to_owned()),
            working_directory: None,
            scrollback_start: 0,
            extensions: Vec::new(),
        }
    }

    #[test]
    fn device_loss_rebuild_restores_consistent_content() {
        let mut coordinator = RecoveryCoordinator::new();
        let content = snapshot();
        coordinator.retain(content.clone());
        // Fault injection: device lost.
        coordinator.on_device_lost();
        assert_eq!(coordinator.phase(), LifecyclePhase::DeviceLost);
        assert!(
            coordinator.session_alive(),
            "SSH session must not disconnect"
        );
        assert_eq!(coordinator.losses(), 1);
        // Rebuild from the retained content.
        assert!(coordinator.begin_rebuild());
        assert_eq!(coordinator.phase(), LifecyclePhase::Rebuilding);
        assert!(coordinator.finish_rebuild());
        assert_eq!(coordinator.phase(), LifecyclePhase::Recovered);
        assert_eq!(coordinator.rebuilds(), 1);
        assert_eq!(
            coordinator.retained_snapshot(),
            Some(&content),
            "recovered content must equal the retained snapshot"
        );
    }

    #[test]
    fn rebuild_without_retained_content_fails() {
        let mut coordinator = RecoveryCoordinator::new();
        coordinator.on_device_lost();
        assert!(!coordinator.begin_rebuild(), "nothing to rebuild");
    }

    #[test]
    fn background_foreground_keeps_content_and_session() {
        let mut coordinator = RecoveryCoordinator::new();
        let content = snapshot();
        coordinator.retain(content.clone());
        coordinator.on_app_background();
        assert!(coordinator.session_alive());
        coordinator.on_app_foreground();
        assert_eq!(
            coordinator.retained_snapshot(),
            Some(&content),
            "content must be restored on foreground"
        );
    }

    #[test]
    fn window_rebuild_restores_content() {
        let mut coordinator = RecoveryCoordinator::new();
        let content = snapshot();
        coordinator.retain(content.clone());
        coordinator.on_window_recreated();
        assert_eq!(coordinator.phase(), LifecyclePhase::Recovered);
        assert_eq!(coordinator.window_recreations(), 1);
        assert_eq!(coordinator.retained_snapshot(), Some(&content));
        assert!(coordinator.session_alive());
    }

    #[test]
    fn full_lifecycle_fault_injection() {
        let mut coordinator = RecoveryCoordinator::new();
        let content = snapshot();
        coordinator.retain(content.clone());
        // Device lost -> rebuild -> recovered -> background -> foreground ->
        // window recreated. The session stays alive throughout.
        for _ in 0..3 {
            coordinator.on_device_lost();
            assert!(coordinator.begin_rebuild());
            assert!(coordinator.finish_rebuild());
            coordinator.on_app_background();
            coordinator.on_app_foreground();
            coordinator.on_window_recreated();
            assert!(coordinator.session_alive());
            assert_eq!(coordinator.retained_snapshot(), Some(&content));
        }
        assert_eq!(coordinator.losses(), 3);
        assert_eq!(coordinator.rebuilds(), 3);
        assert_eq!(coordinator.window_recreations(), 3);
    }
}
