#![forbid(unsafe_code)]

//! Windows PC GUI (clients/windows).
//!
//! The library holds the pure, headless-testable UI model ([`model`]) plus
//! the host-shell identity constants. The native Win32 shell lives in the
//! `onehub` binary (`src/main.rs`): it only renders the model and feeds it
//! keystrokes, so every GUI behavior is verifiable without opening a window.

pub mod ai_assistant;
pub mod docker_tools;
pub mod local_sessions;
pub mod mcp_tools;
pub mod misc_tools;
pub mod model;
pub mod network_diagnostic;
pub mod probe;
pub mod rdp_tools;
pub mod remote_monitor;
pub mod scheduled_tasks;
pub mod sftp;
pub mod ssh_terminal;
pub mod store;
pub mod transfer_bundle;
pub mod tunnels;
pub mod vnc_tools;
pub mod webdav_tools;

/// Platform identity used by diagnostics and architecture tooling.
pub const PLATFORM: &str = "windows";
/// The approved bridge named by `contract.json`.
pub const APPROVED_BRIDGE: &str = "abi-c";
