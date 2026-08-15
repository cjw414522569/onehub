//! Android lifecycle, foreground service, and network-switch model (T127).
//!
//! Models Android's background rules: an active session in the background
//! requires a foreground service, Doze restricts the network, and a network
//! switch (Wi-Fi <-> cellular) must not lose the session. The app never
//! pretends to be permanently online: [`LifecycleModel::is_deceptive`] is
//! true exactly when the UI would claim an active session that cannot be
//! sustained under the current background / Doze / service state.

/// The app lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    /// The app is in the foreground.
    Foreground,
    /// The app is in the background.
    Background,
}

/// The active network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    /// Wi-Fi.
    Wifi,
    /// Cellular.
    Cellular,
    /// No network.
    None,
}

/// The Android lifecycle / foreground-service / network model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleModel {
    /// The app state.
    pub state: AppState,
    /// Whether a session is active.
    pub active_session: bool,
    /// Whether a foreground service is running.
    pub foreground_service: bool,
    /// Whether the device is in Doze.
    pub doze: bool,
    /// The active network.
    pub network: NetworkState,
}

impl LifecycleModel {
    /// A fresh model: foreground, no session.
    pub fn new() -> Self {
        Self {
            state: AppState::Foreground,
            active_session: false,
            foreground_service: false,
            doze: false,
            network: NetworkState::None,
        }
    }

    /// Moves to the foreground.
    pub fn on_foreground(&mut self) {
        self.state = AppState::Foreground;
    }

    /// Moves to the background.
    pub fn on_background(&mut self) {
        self.state = AppState::Background;
    }

    /// Starts a session.
    pub fn start_session(&mut self) {
        self.active_session = true;
    }

    /// Ends a session.
    pub fn end_session(&mut self) {
        self.active_session = false;
    }

    /// Sets the foreground-service state.
    pub fn set_foreground_service(&mut self, running: bool) {
        self.foreground_service = running;
    }

    /// Sets the Doze state.
    pub fn set_doze(&mut self, doze: bool) {
        self.doze = doze;
    }

    /// Sets the active network.
    pub fn set_network(&mut self, network: NetworkState) {
        self.network = network;
    }

    /// Whether a foreground service is required by the system rules (an
    /// active session while the app is in the background).
    pub fn requires_foreground_service(&self) -> bool {
        self.active_session && self.state == AppState::Background
    }

    /// Whether the session can be sustained under the current rules.
    pub fn can_sustain_session(&self) -> bool {
        if self.state == AppState::Foreground {
            return self.network != NetworkState::None;
        }
        // Background: a foreground service is required, and Doze blocks it.
        self.foreground_service && !self.doze && self.network != NetworkState::None
    }

    /// Whether the UI would claim an active session.
    pub fn claims_online(&self) -> bool {
        self.active_session
    }

    /// Whether the app would pretend to be permanently online while it
    /// cannot sustain the session (deceptive).
    pub fn is_deceptive(&self) -> bool {
        self.active_session && !self.can_sustain_session()
    }
}

impl Default for LifecycleModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState, LifecycleModel, NetworkState};

    #[test]
    fn lifecycle_transitions() {
        let mut model = LifecycleModel::new();
        assert_eq!(model.state, AppState::Foreground);
        model.on_background();
        assert_eq!(model.state, AppState::Background);
        model.on_foreground();
        assert_eq!(model.state, AppState::Foreground);
    }

    #[test]
    fn foreground_service_is_required_in_background_with_active_session() {
        let mut model = LifecycleModel::new();
        model.start_session();
        model.set_network(NetworkState::Wifi);
        model.on_background();
        assert!(model.requires_foreground_service());
        // Without the foreground service the session cannot be sustained and
        // the UI would be deceptive.
        assert!(!model.can_sustain_session());
        assert!(model.is_deceptive());
        // Starting the foreground service makes it sustainable.
        model.set_foreground_service(true);
        assert!(model.can_sustain_session());
        assert!(!model.is_deceptive());
    }

    #[test]
    fn doze_blocks_background_sessions() {
        let mut model = LifecycleModel::new();
        model.start_session();
        model.on_background();
        model.set_foreground_service(true);
        model.set_network(NetworkState::Cellular);
        assert!(model.can_sustain_session());
        model.set_doze(true);
        assert!(!model.can_sustain_session(), "Doze restricts the network");
        assert!(model.is_deceptive());
    }

    #[test]
    fn network_switch_keeps_session_and_none_does_not() {
        let mut model = LifecycleModel::new();
        model.start_session();
        model.on_background();
        model.set_foreground_service(true);
        model.set_network(NetworkState::Wifi);
        assert!(model.can_sustain_session());
        model.set_network(NetworkState::Cellular);
        assert!(
            model.can_sustain_session(),
            "network switch must not lose the session"
        );
        model.set_network(NetworkState::None);
        assert!(!model.can_sustain_session());
    }

    #[test]
    fn never_pretends_online_without_a_session() {
        let mut model = LifecycleModel::new();
        model.on_background();
        model.set_doze(true);
        model.set_network(NetworkState::None);
        assert!(!model.is_deceptive(), "no session, no false online claim");
        assert!(!model.claims_online());
    }
}
