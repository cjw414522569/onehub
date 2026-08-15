//! iOS / iPadOS lifecycle, scenes, multi-window, and suspension model (T129).
//!
//! Models scene lifecycle (active / inactive / background), a multi-scene
//! (multi-window) collection, truthful system-suspension limits (the app
//! never claims an active session while iOS would suspend it), and
//! recovery-state consistency after relaunch. Real simulator + device
//! lifecycle tests run on Apple hosts.

/// The scene state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneState {
    /// The scene is active.
    Active,
    /// The scene is inactive (e.g. transient interruption).
    Inactive,
    /// The scene is in the background.
    Background,
}

/// A scene (window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// Stable id.
    pub id: u64,
    /// Title.
    pub title: String,
    /// The state.
    pub state: SceneState,
}

/// The multi-scene collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCollection {
    /// The scenes.
    pub scenes: Vec<Scene>,
    /// The active scene index.
    pub active_scene: usize,
}

impl SceneCollection {
    /// A collection with one active scene.
    pub fn new() -> Self {
        Self {
            scenes: vec![Scene {
                id: 1,
                title: "Main".to_owned(),
                state: SceneState::Active,
            }],
            active_scene: 0,
        }
    }

    /// Adds a scene; returns its id.
    pub fn add_scene(&mut self) -> u64 {
        let id = self.scenes.iter().map(|scene| scene.id).max().unwrap_or(0) + 1;
        self.scenes.push(Scene {
            id,
            title: format!("Scene {}", id),
            state: SceneState::Inactive,
        });
        id
    }

    /// Closes a scene; focus moves to a valid scene.
    pub fn close_scene(&mut self, index: usize) -> bool {
        if index >= self.scenes.len() {
            return false;
        }
        self.scenes.remove(index);
        if self.scenes.is_empty() {
            self.scenes.push(Scene {
                id: 1,
                title: "Main".to_owned(),
                state: SceneState::Active,
            });
            self.active_scene = 0;
        } else {
            self.active_scene = self.active_scene.min(self.scenes.len() - 1);
        }
        true
    }

    /// Activates a scene.
    pub fn activate(&mut self, index: usize) -> bool {
        if index >= self.scenes.len() {
            return false;
        }
        self.active_scene = index;
        true
    }
}

impl Default for SceneCollection {
    fn default() -> Self {
        Self::new()
    }
}

/// The truthful suspension model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuspensionModel {
    /// Whether the app is in the foreground.
    pub foreground: bool,
    /// Whether a background task is running.
    pub background_task: bool,
    /// Whether a session is active.
    pub active_session: bool,
}

impl SuspensionModel {
    /// A fresh model in the foreground with no session.
    pub fn new() -> Self {
        Self {
            foreground: true,
            background_task: false,
            active_session: false,
        }
    }

    /// Moves to the foreground.
    pub fn on_foreground(&mut self) {
        self.foreground = true;
    }

    /// Moves to the background.
    pub fn on_background(&mut self) {
        self.foreground = false;
    }

    /// Starts a background task.
    pub fn start_background_task(&mut self) {
        self.background_task = true;
    }

    /// Ends a background task.
    pub fn end_background_task(&mut self) {
        self.background_task = false;
    }

    /// Starts a session.
    pub fn start_session(&mut self) {
        self.active_session = true;
    }

    /// Whether the app may claim an active session (foreground or a running
    /// background task).
    pub fn may_claim_active(&self) -> bool {
        self.active_session && (self.foreground || self.background_task)
    }

    /// Whether iOS would suspend the app (background, no background task).
    pub fn suspended(&self) -> bool {
        !self.foreground && !self.background_task
    }

    /// Whether the UI would deceive (claim active while suspended).
    pub fn is_deceptive(&self) -> bool {
        self.active_session && self.suspended()
    }
}

impl Default for SuspensionModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Recovery state after a relaunch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryState {
    /// Sessions saved before suspension.
    pub saved_sessions: Vec<u64>,
    /// Sessions restored after relaunch.
    pub restored_sessions: Vec<u64>,
}

impl RecoveryState {
    /// Restores the saved sessions (all of them).
    pub fn restore(&mut self, saved: Vec<u64>) {
        self.saved_sessions = saved.clone();
        self.restored_sessions = saved;
    }

    /// Whether the restored state matches the saved state.
    pub fn consistent(&self) -> bool {
        self.saved_sessions == self.restored_sessions
    }
}

#[cfg(test)]
mod tests {
    use super::{RecoveryState, SceneCollection, SuspensionModel};

    #[test]
    fn scene_collection_multi_window() {
        let mut collection = SceneCollection::new();
        assert_eq!(collection.scenes.len(), 1);
        collection.add_scene();
        collection.add_scene();
        assert_eq!(collection.scenes.len(), 3);
        assert!(collection.activate(2));
        assert_eq!(collection.active_scene, 2);
        assert!(collection.close_scene(2));
        assert_eq!(collection.scenes.len(), 2);
        // Closing all scenes recreates the main scene.
        collection.close_scene(0);
        collection.close_scene(0);
        assert_eq!(collection.scenes.len(), 1);
    }

    #[test]
    fn suspension_limits_are_truthful() {
        let mut model = SuspensionModel::new();
        model.start_session();
        model.on_background();
        // Background without a background task: iOS suspends the app and the
        // UI must not claim an active session.
        assert!(model.suspended());
        assert!(!model.may_claim_active());
        assert!(model.is_deceptive());
        // Starting a background task makes the claim truthful.
        model.start_background_task();
        assert!(model.may_claim_active());
        assert!(!model.is_deceptive());
        model.end_background_task();
        // Returning to the foreground is truthful again.
        model.on_foreground();
        assert!(model.may_claim_active());
        assert!(!model.is_deceptive());
    }

    #[test]
    fn recovery_state_is_consistent() {
        let mut recovery = RecoveryState {
            saved_sessions: Vec::new(),
            restored_sessions: Vec::new(),
        };
        recovery.restore(vec![1, 2, 3]);
        assert!(recovery.consistent());
        assert_eq!(recovery.restored_sessions, vec![1, 2, 3]);
    }
}
