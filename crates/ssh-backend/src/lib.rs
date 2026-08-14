#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # ssh-backend
//!
//! SSH backend adapter boundary. The domain and orchestration layers depend
//! on the injectable [`SshTransport`] contract and never reference concrete
//! backend-library types (russh/libssh/OpenSSH), so they can be tested with
//! a fake transport.

pub mod agent;
pub mod algorithms;
pub mod authentication;
pub mod channel_qos;
pub mod connectivity;
pub mod hardware_key;
pub mod host_key_verify;
pub mod keepalive;
pub mod known_hosts;
pub mod private_key;
pub mod session_channel;
pub mod transport;
pub mod user_certificate;

pub use agent::{
    frame_message, parse_frame, AgentClient, AgentError, AgentIdentity, AgentStream,
    FakeAgentServer, AGENT_MAX_MESSAGE_LEN, SSH_AGENT_CONSTRAIN_CONFIRM,
};
pub use algorithms::{
    negotiate_algorithm, Algorithm, AlgorithmKind, AlgorithmPolicy, AlgorithmSecurity,
    HostAlgorithmPolicy, NegotiatedAlgorithm, SshVersion, SshVersionError,
};
pub use authentication::{
    run_keyboard_interactive, AuthChallenge, AuthFailure, AuthMethod, AuthOutcome, AuthPrompt,
    AuthResponse, KeyboardInteractiveHandler, PasswordAuthenticator,
};
pub use channel_qos::{
    ChannelSnapshot, FlowWindow, QosError, ScheduledSend, Scheduler, SchedulerConfig,
    SchedulerSnapshot, TrafficClass,
};
pub use connectivity::{
    happy_eyeballs_connect, ConnectError, ConnectOutcome, ConnectionGuard, Connector,
    CountingConnector, ResolveError, ResolvedAddresses, Resolver, StaticResolver,
    HAPPY_EYEBALLS_START_DELAY,
};
pub use hardware_key::{
    effective_gate, hardware_key_gate, HardwareKeyBackend, HardwareKeyGate, HardwareKeyKind,
};
pub use host_key_verify::{
    CaStore, HostCertificate, HostKeyFingerprint, HostKeyVerification, HostKeyVerifier,
};
pub use keepalive::{
    probe_with_timeout, run_reconnect_loop, KeepaliveConfig, LivenessProbe, MonitorState,
    MonitorStateHandle, ProbeError, ReconnectBackoff,
};
pub use known_hosts::{
    hashed_host_matches, host_field_matches, hostname_matches_pattern, KnownHostsStore,
};
pub use private_key::{
    detect_format, load_private_key, KeyAlgorithm, KeyError, PrivateKeyFormat, PrivateKeyHandle,
};
pub use session_channel::{
    run_exec_command, run_interactive_shell, ChannelError, ChannelEvent, ExecCommand, ExitStatus,
    PtyConfig, ScriptedChannel, SessionChannel,
};
pub use transport::{
    FakeTransport, FakeTransportMode, SessionHandle, SessionTarget, SshTransport, TransportError,
};
pub use user_certificate::{ed25519_ca_fingerprint, verify_user_certificate, UserCertVerification};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "ssh-backend";
