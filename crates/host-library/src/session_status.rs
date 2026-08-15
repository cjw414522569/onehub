//! Session state, latency, reconnect, and read-only indicators (T111).
//!
//! Every session state is recognized by a **non-color** indicator: a glyph,
//! a label, a human description, and a visual pattern (solid / dashed /
//! hatched / animated / hollow / blinking). [`SessionStatusModel`] is a
//! validated state machine over [`SessionState`], and latency is shown with
//! text (e.g. "12 ms") and a quality label, never by color alone.

/// The session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// No active session.
    Disconnected,
    /// Connection in progress.
    Connecting,
    /// Connected and interactive.
    Connected,
    /// Reconnecting after a drop.
    Reconnecting,
    /// Connected in read-only mode.
    ReadOnly,
    /// A connection error occurred.
    Error,
    /// The session was closed (terminal).
    Closed,
}

/// A non-color visual pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndicatorPattern {
    /// Solid fill.
    Solid,
    /// Dashed outline.
    Dashed,
    /// Hatched fill.
    Hatched,
    /// Animated (spinner).
    Animated,
    /// Hollow outline.
    Hollow,
    /// Blinking.
    Blinking,
}

/// A state indicator that is recognizable without color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateIndicator {
    /// A glyph.
    pub glyph: &'static str,
    /// A short label.
    pub label: &'static str,
    /// A longer description.
    pub description: String,
    /// A non-color visual pattern.
    pub pattern: IndicatorPattern,
}

/// Latency quality (text, not color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyQuality {
    /// Unknown / not measured.
    Unknown,
    /// Fast.
    Good,
    /// Acceptable.
    Ok,
    /// Slow.
    Slow,
}

/// The live session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStatus {
    /// The state.
    pub state: SessionState,
    /// Measured round-trip latency.
    pub latency_ms: Option<u64>,
    /// Reconnect attempts since the last stable connection.
    pub reconnect_attempts: u32,
}

/// Why a state transition was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateError {
    /// The source state.
    pub from: SessionState,
    /// The requested state.
    pub to: SessionState,
}

/// The session status model (validated state machine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStatusModel {
    status: SessionStatus,
    error_message: Option<String>,
}

impl SessionStatusModel {
    /// A model starting disconnected.
    pub fn new() -> Self {
        Self {
            status: SessionStatus {
                state: SessionState::Disconnected,
                latency_ms: None,
                reconnect_attempts: 0,
            },
            error_message: None,
        }
    }

    /// The current status.
    pub fn status(&self) -> &SessionStatus {
        &self.status
    }

    /// The last error message (when in [`SessionState::Error`]).
    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    fn transition(&mut self, to: SessionState) -> Result<(), StateError> {
        let from = self.status.state;
        let allowed = matches!(
            (from, to),
            (SessionState::Disconnected, SessionState::Connecting)
                | (SessionState::Connecting, SessionState::Connected)
                | (SessionState::Connecting, SessionState::Error)
                | (SessionState::Connecting, SessionState::Disconnected)
                | (SessionState::Connected, SessionState::ReadOnly)
                | (SessionState::Connected, SessionState::Disconnected)
                | (SessionState::Connected, SessionState::Error)
                | (SessionState::Connected, SessionState::Reconnecting)
                | (SessionState::Connected, SessionState::Closed)
                | (SessionState::ReadOnly, SessionState::Connected)
                | (SessionState::ReadOnly, SessionState::Disconnected)
                | (SessionState::ReadOnly, SessionState::Error)
                | (SessionState::ReadOnly, SessionState::Reconnecting)
                | (SessionState::ReadOnly, SessionState::Closed)
                | (SessionState::Reconnecting, SessionState::Connected)
                | (SessionState::Reconnecting, SessionState::Error)
                | (SessionState::Reconnecting, SessionState::Closed)
                | (SessionState::Error, SessionState::Connecting)
                | (SessionState::Error, SessionState::Closed)
        );
        if allowed {
            self.status.state = to;
            if to != SessionState::Error {
                self.error_message = None;
            }
            Ok(())
        } else {
            Err(StateError { from, to })
        }
    }

    /// Begins a connection.
    pub fn connect(&mut self) -> Result<(), StateError> {
        self.transition(SessionState::Connecting)
    }

    /// The connection is established (stable: resets the reconnect count).
    pub fn on_connected(&mut self) -> Result<(), StateError> {
        self.transition(SessionState::Connected)?;
        self.status.reconnect_attempts = 0;
        Ok(())
    }

    /// The connection dropped: reconnect (or go disconnected).
    pub fn on_disconnected(&mut self) -> Result<(), StateError> {
        if self.status.state == SessionState::Connected
            || self.status.state == SessionState::ReadOnly
        {
            self.status.reconnect_attempts += 1;
            self.transition(SessionState::Reconnecting)
        } else {
            self.transition(SessionState::Disconnected)
        }
    }

    /// A connection error occurred.
    pub fn on_error(&mut self, message: impl Into<String>) -> Result<(), StateError> {
        if self.status.state == SessionState::Reconnecting {
            self.status.reconnect_attempts += 1;
        }
        self.error_message = Some(message.into());
        self.transition(SessionState::Error)
    }

    /// Toggles read-only mode.
    pub fn set_read_only(&mut self, read_only: bool) -> Result<(), StateError> {
        match (self.status.state, read_only) {
            (SessionState::Connected, true) => self.transition(SessionState::ReadOnly),
            (SessionState::ReadOnly, false) => self.transition(SessionState::Connected),
            _ => Err(StateError {
                from: self.status.state,
                to: if read_only {
                    SessionState::ReadOnly
                } else {
                    SessionState::Connected
                },
            }),
        }
    }

    /// Closes the session (terminal state).
    pub fn close(&mut self) -> Result<(), StateError> {
        if self.status.state == SessionState::Closed {
            return Err(StateError {
                from: SessionState::Closed,
                to: SessionState::Closed,
            });
        }
        self.transition(SessionState::Closed)
    }

    /// Records a measured latency.
    pub fn set_latency(&mut self, latency_ms: u64) {
        self.status.latency_ms = Some(latency_ms);
    }

    /// The non-color indicator for the current state.
    pub fn indicator(&self) -> StateIndicator {
        match self.status.state {
            SessionState::Disconnected => StateIndicator {
                glyph: "○",
                label: "Disconnected",
                description: "No active session.".to_owned(),
                pattern: IndicatorPattern::Hollow,
            },
            SessionState::Connecting => StateIndicator {
                glyph: "◌",
                label: "Connecting",
                description: "Establishing the connection…".to_owned(),
                pattern: IndicatorPattern::Animated,
            },
            SessionState::Connected => StateIndicator {
                glyph: "●",
                label: "Connected",
                description: "Session active.".to_owned(),
                pattern: IndicatorPattern::Solid,
            },
            SessionState::Reconnecting => StateIndicator {
                glyph: "↻",
                label: "Reconnecting",
                description: format!("Reconnecting (attempt {}).", self.status.reconnect_attempts),
                pattern: IndicatorPattern::Dashed,
            },
            SessionState::ReadOnly => StateIndicator {
                glyph: "⛉",
                label: "Read-only",
                description: "The session is read-only.".to_owned(),
                pattern: IndicatorPattern::Hatched,
            },
            SessionState::Error => StateIndicator {
                glyph: "⚠",
                label: "Error",
                description: self
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "A connection error occurred.".to_owned()),
                pattern: IndicatorPattern::Blinking,
            },
            SessionState::Closed => StateIndicator {
                glyph: "×",
                label: "Closed",
                description: "The session was closed.".to_owned(),
                pattern: IndicatorPattern::Hollow,
            },
        }
    }

    /// A text latency label (never color alone).
    pub fn latency_label(&self) -> String {
        match self.status.latency_ms {
            Some(ms) if ms >= 1000 => format!("{} s", ms as f64 / 1000.0),
            Some(ms) => format!("{ms} ms"),
            None => "—".to_owned(),
        }
    }

    /// The latency quality as text.
    pub fn latency_quality(&self) -> LatencyQuality {
        match self.status.latency_ms {
            None => LatencyQuality::Unknown,
            Some(ms) if ms < 50 => LatencyQuality::Good,
            Some(ms) if ms < 250 => LatencyQuality::Ok,
            Some(_) => LatencyQuality::Slow,
        }
    }
}

impl Default for SessionStatusModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IndicatorPattern, LatencyQuality, SessionState, SessionStatusModel, StateError,
        StateIndicator,
    };

    fn indicator_for(model: &SessionStatusModel) -> StateIndicator {
        model.indicator()
    }

    #[test]
    fn state_machine_valid_transitions() {
        let mut model = SessionStatusModel::new();
        assert_eq!(model.status().state, SessionState::Disconnected);
        model.connect().unwrap();
        assert_eq!(model.status().state, SessionState::Connecting);
        model.on_connected().unwrap();
        assert_eq!(model.status().state, SessionState::Connected);
        model.set_read_only(true).unwrap();
        assert_eq!(model.status().state, SessionState::ReadOnly);
        model.set_read_only(false).unwrap();
        assert_eq!(model.status().state, SessionState::Connected);
        model.on_disconnected().unwrap();
        assert_eq!(model.status().state, SessionState::Reconnecting);
        assert_eq!(model.status().reconnect_attempts, 1);
        model.on_connected().unwrap();
        assert_eq!(model.status().state, SessionState::Connected);
        model.close().unwrap();
        assert_eq!(model.status().state, SessionState::Closed);
        // Terminal state: further transitions are refused.
        assert_eq!(
            model.connect(),
            Err(StateError {
                from: SessionState::Closed,
                to: SessionState::Connecting
            })
        );
    }

    #[test]
    fn error_path_records_message_and_attempts() {
        let mut model = SessionStatusModel::new();
        model.connect().unwrap();
        model.on_error("host key rejected").unwrap();
        assert_eq!(model.status().state, SessionState::Error);
        assert_eq!(model.error_message(), Some("host key rejected"));
        // Retry: connect, drop into reconnect, then fail again; attempts grow.
        model.connect().unwrap();
        model.on_connected().unwrap();
        model.on_disconnected().unwrap();
        assert_eq!(model.status().state, SessionState::Reconnecting);
        assert_eq!(model.status().reconnect_attempts, 1);
        model.on_error("timeout").unwrap();
        assert_eq!(model.status().state, SessionState::Error);
        assert_eq!(model.status().reconnect_attempts, 2);
        assert_eq!(model.error_message(), Some("timeout"));
    }

    #[test]
    fn every_state_has_a_non_color_indicator() {
        let states = [
            SessionState::Disconnected,
            SessionState::Connecting,
            SessionState::Connected,
            SessionState::Reconnecting,
            SessionState::ReadOnly,
            SessionState::Error,
            SessionState::Closed,
        ];
        let mut seen = std::collections::HashSet::new();
        for state in states {
            let mut model = SessionStatusModel::new();
            // Drive the model into the target state.
            drive(&mut model, state);
            let indicator = indicator_for(&model);
            assert!(!indicator.glyph.is_empty(), "{state:?} needs a glyph");
            assert!(!indicator.label.is_empty(), "{state:?} needs a label");
            assert!(
                !indicator.description.is_empty(),
                "{state:?} needs a description"
            );
            // Unique (glyph, pattern) pair: distinguishable beyond color.
            let key = (indicator.glyph, indicator.pattern);
            assert!(
                seen.insert(key),
                "{state:?} duplicates indicator pair {key:?}"
            );
        }
    }

    fn drive(model: &mut SessionStatusModel, target: SessionState) {
        match target {
            SessionState::Disconnected => {}
            SessionState::Connecting => {
                model.connect().unwrap();
            }
            SessionState::Connected => {
                model.connect().unwrap();
                model.on_connected().unwrap();
            }
            SessionState::Reconnecting => {
                model.connect().unwrap();
                model.on_connected().unwrap();
                model.on_disconnected().unwrap();
            }
            SessionState::ReadOnly => {
                model.connect().unwrap();
                model.on_connected().unwrap();
                model.set_read_only(true).unwrap();
            }
            SessionState::Error => {
                model.connect().unwrap();
                model.on_error("boom").unwrap();
            }
            SessionState::Closed => {
                model.connect().unwrap();
                model.on_connected().unwrap();
                model.close().unwrap();
            }
        }
    }

    #[test]
    fn latency_is_shown_as_text_not_color() {
        let mut model = SessionStatusModel::new();
        assert_eq!(model.latency_label(), "—");
        assert_eq!(model.latency_quality(), LatencyQuality::Unknown);
        model.set_latency(12);
        assert_eq!(model.latency_label(), "12 ms");
        assert_eq!(model.latency_quality(), LatencyQuality::Good);
        model.set_latency(120);
        assert_eq!(model.latency_quality(), LatencyQuality::Ok);
        model.set_latency(1500);
        assert_eq!(model.latency_label(), "1.5 s");
        assert_eq!(model.latency_quality(), LatencyQuality::Slow);
    }

    #[test]
    fn read_only_and_reconnect_indicators_differ() {
        let mut connected = SessionStatusModel::new();
        connected.connect().unwrap();
        connected.on_connected().unwrap();
        let solid = indicator_for(&connected);
        assert_eq!(solid.pattern, IndicatorPattern::Solid);
        connected.set_read_only(true).unwrap();
        let read_only = indicator_for(&connected);
        assert_eq!(read_only.pattern, IndicatorPattern::Hatched);
        assert_eq!(read_only.label, "Read-only");
    }
}
