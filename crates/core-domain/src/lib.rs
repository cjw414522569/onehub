#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # core-domain
//!
//! Pure, immutable domain values for the multi-platform SSH client.
//! This crate contains no UI, database, or SSH implementation types.

pub mod command;
pub mod credential;
pub mod credential_provider;
pub mod error;
pub mod forwarding;
pub mod host;
pub mod host_key;
pub mod proxy_chain;
pub mod session;
pub mod settings;
pub mod transfer;

pub use command::{
    resolve_command, CommandSnippet, EnvVar, Environment, Macro, PlaceholderDef, ResolvedCommand,
};
pub use credential::{CredentialKind, CredentialRef};
pub use credential_provider::{
    AgentHandle, CredentialProvider, CredentialValue, HardwareKeyHandle, ProviderError,
    UnlockInteraction,
};
pub use error::DomainError;
pub use forwarding::{
    ForwardingAddResult, ForwardingEndpoint, ForwardingFamily, ForwardingKind, ForwardingSpec,
    ForwardingTable,
};
pub use host::{Host, HostAddress, HostId, DEFAULT_SSH_PORT};
pub use host_key::{
    verify_host_key, HostKeyDecision, HostKeyIdentity, HostKeyPolicy, HostKeyStatus,
    KnownHostsEntry, KnownHostsMarker,
};
pub use proxy_chain::{
    proxy_jump, proxy_jump_multi, AddressFamily, ChainValidation, HopPolicy, ProxyChain, ProxyHop,
    ProxyKind,
};
pub use session::SessionProfile;
pub use settings::{
    migrate_settings, LayoutNode, LocalSettings, SettingsDocument, SettingsEntry, SettingsScope,
    SettingsValue, SplitDirection, Tab, TabId, WindowLayout, Workspace, SETTINGS_SCHEMA_VERSION,
};
pub use transfer::{
    RemoteFileOp, TransferDirection, TransferError, TransferMode, TransferProgress, TransferSpec,
    TransferStatus,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "core-domain";
