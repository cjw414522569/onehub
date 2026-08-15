#![forbid(unsafe_code)]

//! Windows PC GUI (clients/windows).
//!
//! The library holds the pure, headless-testable UI model ([`model`]) plus
//! the host-shell identity constants. The native Win32 shell lives in the
//! `ssh-gui` binary (`src/main.rs`): it only renders the model and feeds it
//! keystrokes, so every GUI behavior is verifiable without opening a window.

pub mod model;
pub mod probe;
pub mod sftp;
pub mod store;

/// Platform identity used by diagnostics and architecture tooling.
pub const PLATFORM: &str = "windows";
/// The approved bridge named by `contract.json`.
pub const APPROVED_BRIDGE: &str = "abi-c";
