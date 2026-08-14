use serde::{Deserialize, Serialize};

/// Lifecycle state of a session.
///
/// The set is closed: connect → authenticate → online, with reconnect and
/// suspend branches, and a terminal `Closed`. Every state/event pair has a
/// deterministic outcome (accepted transition or explicit rejection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionState {
    /// No connection is active or being established.
    Disconnected,
    /// A TCP/SSH connection attempt is in flight.
    Connecting,
    /// The transport is established; authentication is in flight.
    Authenticating,
    /// The session is usable.
    Online,
    /// The transport dropped; an automatic reconnect is pending or in flight.
    Reconnecting,
    /// The session is paused (e.g. mobile background), transport retained if possible.
    Suspended,
    /// A close was requested and is being drained.
    Closing,
    /// Terminal state; no further transitions are accepted.
    Closed,
}

/// A domain event applied to the session state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionEvent {
    /// User or caller asked to open the session.
    ConnectRequested,
    /// The transport connected.
    Connected,
    /// Authentication completed successfully.
    Authenticated,
    /// The transport dropped unexpectedly.
    Disconnected,
    /// An automatic reconnect attempt started.
    ReconnectStarted,
    /// The platform suspended the session.
    Suspended,
    /// The platform resumed the session.
    Resumed,
    /// User or caller asked to close the session.
    CloseRequested,
    /// The close sequence finished.
    Closed,
    /// Connection or authentication failed.
    ConnectFailed,
}

/// A command emitted by the state machine for the orchestrator to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionEffect {
    /// Begin establishing the transport.
    StartConnect,
    /// Begin authentication.
    StartAuthentication,
    /// Surface the online state to the UI.
    NotifyOnline,
    /// Schedule an automatic reconnect.
    ScheduleReconnect,
    /// Surface the reconnect state to the UI.
    NotifyReconnecting,
    /// Surface the suspended state to the UI.
    NotifySuspended,
    /// Surface the resumed state to the UI.
    NotifyResumed,
    /// Begin draining and closing.
    StartClose,
    /// Surface the closed state to the UI.
    NotifyClosed,
}

/// The outcome of applying an event to a state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTransition {
    /// The resulting state.
    pub state: SessionState,
    /// Effects the orchestrator should execute.
    pub effects: Vec<SessionEffect>,
}

/// Result of `apply`: an accepted transition or an explicit rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTransitionResult {
    /// The event was accepted and produced a transition.
    Accepted(SessionTransition),
    /// The event is invalid for the current state.
    Rejected {
        /// The state the event was applied to.
        state: SessionState,
        /// The rejected event.
        event: SessionEvent,
    },
}

impl SessionTransitionResult {
    /// Returns the transition if accepted.
    pub fn accepted(self) -> Option<SessionTransition> {
        match self {
            SessionTransitionResult::Accepted(transition) => Some(transition),
            SessionTransitionResult::Rejected { .. } => None,
        }
    }

    /// Returns whether the event was rejected.
    pub fn is_rejected(&self) -> bool {
        matches!(self, SessionTransitionResult::Rejected { .. })
    }
}

fn transition(state: SessionState, effects: &[SessionEffect]) -> SessionTransitionResult {
    SessionTransitionResult::Accepted(SessionTransition {
        state,
        effects: effects.to_vec(),
    })
}

/// Applies an event to a state, returning a deterministic transition or an
/// explicit rejection. Pure and side-effect free (event sourcing).
pub fn apply(state: SessionState, event: SessionEvent) -> SessionTransitionResult {
    use SessionEffect as Eff;
    use SessionEvent as Ev;
    use SessionState as St;
    match (state, event) {
        (St::Disconnected, Ev::ConnectRequested) => {
            transition(St::Connecting, &[Eff::StartConnect])
        }
        (St::Disconnected, Ev::CloseRequested) => transition(St::Closing, &[Eff::StartClose]),
        (St::Connecting, Ev::Connected) => {
            transition(St::Authenticating, &[Eff::StartAuthentication])
        }
        (St::Connecting, Ev::ConnectFailed) => {
            transition(St::Disconnected, &[Eff::ScheduleReconnect])
        }
        (St::Connecting, Ev::Disconnected) => {
            transition(St::Disconnected, &[Eff::ScheduleReconnect])
        }
        (St::Connecting, Ev::CloseRequested) => transition(St::Closing, &[Eff::StartClose]),
        (St::Authenticating, Ev::Authenticated) => transition(St::Online, &[Eff::NotifyOnline]),
        (St::Authenticating, Ev::ConnectFailed) => {
            transition(St::Disconnected, &[Eff::ScheduleReconnect])
        }
        (St::Authenticating, Ev::Disconnected) => transition(
            St::Reconnecting,
            &[Eff::NotifyReconnecting, Eff::ScheduleReconnect],
        ),
        (St::Authenticating, Ev::CloseRequested) => transition(St::Closing, &[Eff::StartClose]),
        (St::Online, Ev::Disconnected) => transition(
            St::Reconnecting,
            &[Eff::NotifyReconnecting, Eff::ScheduleReconnect],
        ),
        (St::Online, Ev::Suspended) => transition(St::Suspended, &[Eff::NotifySuspended]),
        (St::Online, Ev::CloseRequested) => transition(St::Closing, &[Eff::StartClose]),
        (St::Reconnecting, Ev::ReconnectStarted) => {
            transition(St::Connecting, &[Eff::StartConnect])
        }
        (St::Reconnecting, Ev::Suspended) => transition(St::Suspended, &[Eff::NotifySuspended]),
        (St::Reconnecting, Ev::CloseRequested) => transition(St::Closing, &[Eff::StartClose]),
        (St::Suspended, Ev::Resumed) => transition(St::Online, &[Eff::NotifyResumed]),
        (St::Suspended, Ev::CloseRequested) => transition(St::Closing, &[Eff::StartClose]),
        (St::Closing, Ev::Closed) => transition(St::Closed, &[Eff::NotifyClosed]),
        (St::Closing, Ev::Disconnected) => transition(St::Closing, &[]),
        (St::Closing, Ev::CloseRequested) => transition(St::Closing, &[]),
        _ => SessionTransitionResult::Rejected { state, event },
    }
}

/// A point-in-time snapshot produced by replaying an event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// The resulting state.
    pub state: SessionState,
    /// Number of events successfully applied.
    pub version: usize,
}

/// Replays an event log from `Disconnected`, stopping at the first rejected
/// event. Returns the snapshot, or the rejected event with its position.
pub fn replay(events: &[SessionEvent]) -> Result<SessionSnapshot, ReplayError> {
    replay_from(SessionState::Disconnected, events)
}

/// Replays an event log from an explicit initial state.
pub fn replay_from(
    initial: SessionState,
    events: &[SessionEvent],
) -> Result<SessionSnapshot, ReplayError> {
    let mut state = initial;
    for (index, event) in events.iter().copied().enumerate() {
        match apply(state, event) {
            SessionTransitionResult::Accepted(transition) => state = transition.state,
            SessionTransitionResult::Rejected { .. } => {
                return Err(ReplayError {
                    index,
                    event,
                    state,
                });
            }
        }
    }
    Ok(SessionSnapshot {
        state,
        version: events.len(),
    })
}

/// Why replay stopped before consuming every event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayError {
    /// Index of the rejected event.
    pub index: usize,
    /// The rejected event.
    pub event: SessionEvent,
    /// The state the rejected event was applied to.
    pub state: SessionState,
}

#[cfg(test)]
mod tests {
    use super::{
        apply, replay, replay_from, ReplayError, SessionEffect, SessionEvent, SessionState,
        SessionTransitionResult,
    };
    use SessionEvent as Ev;
    use SessionState as St;

    fn states() -> Vec<SessionState> {
        vec![
            St::Disconnected,
            St::Connecting,
            St::Authenticating,
            St::Online,
            St::Reconnecting,
            St::Suspended,
            St::Closing,
            St::Closed,
        ]
    }

    fn events() -> Vec<SessionEvent> {
        vec![
            Ev::ConnectRequested,
            Ev::Connected,
            Ev::Authenticated,
            Ev::Disconnected,
            Ev::ReconnectStarted,
            Ev::Suspended,
            Ev::Resumed,
            Ev::CloseRequested,
            Ev::Closed,
            Ev::ConnectFailed,
        ]
    }

    #[test]
    fn acceptance_flow_is_unambiguous() {
        let flow = [
            Ev::ConnectRequested, // Disconnected -> Connecting
            Ev::Connected,        // Connecting -> Authenticating
            Ev::Authenticated,    // Authenticating -> Online
            Ev::Suspended,        // Online -> Suspended
            Ev::Resumed,          // Suspended -> Online
            Ev::Disconnected,     // Online -> Reconnecting
            Ev::ReconnectStarted, // Reconnecting -> Connecting
            Ev::Connected,        // Connecting -> Authenticating
            Ev::Authenticated,    // Authenticating -> Online
            Ev::CloseRequested,   // Online -> Closing
            Ev::Closed,           // Closing -> Closed
        ];
        let snapshot = replay(&flow).expect("acceptance flow must replay");
        assert_eq!(snapshot.state, St::Closed);
        assert_eq!(snapshot.version, flow.len());
    }

    #[test]
    fn every_state_event_pair_has_a_deterministic_outcome() {
        // The machine is total: every pair is either accepted or explicitly
        // rejected, never panics, never undefined.
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        for state in states() {
            for event in events() {
                match apply(state, event) {
                    SessionTransitionResult::Accepted(transition) => {
                        accepted += 1;
                        assert!(
                            transition.state != state || state == St::Closing,
                            "accepted transition must change state unless already draining: {state:?} + {event:?}"
                        );
                        assert!(
                            !transition.effects.is_empty() || state == St::Closing,
                            "accepted transitions emit effects except while draining"
                        );
                    }
                    SessionTransitionResult::Rejected { state: s, event: e } => {
                        rejected += 1;
                        assert_eq!(s, state);
                        assert_eq!(e, event);
                    }
                }
            }
        }
        assert_eq!(
            accepted, 21,
            "expected 21 accepted transitions, got {accepted}"
        );
        assert_eq!(rejected, states().len() * events().len() - 21);
    }

    #[test]
    fn closed_is_terminal_and_rejects_every_event() {
        for event in events() {
            let result = apply(St::Closed, event);
            assert!(
                result.is_rejected(),
                "Closed must reject every event, got {result:?}"
            );
        }
    }

    #[test]
    fn replay_stops_at_first_rejected_event() {
        let flow = [
            Ev::ConnectRequested, // ok
            Ev::Authenticated,    // invalid from Connecting -> replay stops here
            Ev::Connected,
        ];
        let error: ReplayError = replay(&flow).expect_err("replay must stop at the invalid event");
        assert_eq!(error.index, 1);
        assert_eq!(error.event, Ev::Authenticated);
        assert_eq!(error.state, St::Connecting);
    }

    #[test]
    fn replay_is_deterministic_and_pure() {
        let flow = [
            Ev::ConnectRequested,
            Ev::Connected,
            Ev::Authenticated,
            Ev::Disconnected,
            Ev::ReconnectStarted,
            Ev::Connected,
            Ev::Authenticated,
            Ev::CloseRequested,
            Ev::Closed,
        ];
        let first = replay(&flow).expect("first replay");
        let second = replay(&flow).expect("second replay");
        assert_eq!(first, second);
    }

    #[test]
    fn rejected_events_never_mutate_state() {
        let state = St::Online;
        let result = apply(state, Ev::ConnectRequested);
        assert!(result.is_rejected());
        let snapshot = replay_from(state, &[Ev::ConnectRequested]).expect_err("rejected");
        assert_eq!(snapshot.state, St::Online);
    }

    #[test]
    fn reconnect_and_failure_paths_have_explicit_effects() {
        let transition = apply(St::Online, Ev::Disconnected)
            .accepted()
            .expect("accepted");
        assert_eq!(transition.state, St::Reconnecting);
        assert!(transition
            .effects
            .contains(&SessionEffect::ScheduleReconnect));
        assert!(transition
            .effects
            .contains(&SessionEffect::NotifyReconnecting));

        let failed = apply(St::Connecting, Ev::ConnectFailed)
            .accepted()
            .expect("accepted");
        assert_eq!(failed.state, St::Disconnected);
        assert!(failed.effects.contains(&SessionEffect::ScheduleReconnect));
    }

    #[test]
    fn serde_round_trip_for_states_events_and_effects() {
        let json = serde_json::to_string(&St::Reconnecting).expect("serialize state");
        let decoded: SessionState = serde_json::from_str(&json).expect("deserialize state");
        assert_eq!(decoded, St::Reconnecting);

        let json = serde_json::to_string(&Ev::Authenticated).expect("serialize event");
        let decoded: SessionEvent = serde_json::from_str(&json).expect("deserialize event");
        assert_eq!(decoded, Ev::Authenticated);

        let json = serde_json::to_string(&SessionEffect::NotifyOnline).expect("serialize effect");
        let decoded: SessionEffect = serde_json::from_str(&json).expect("deserialize effect");
        assert_eq!(decoded, SessionEffect::NotifyOnline);
    }
}
