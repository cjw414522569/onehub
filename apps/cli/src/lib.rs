#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # cli
//!
//! CLI entrypoint (T101 startup / database unlock / failure-recovery flow;
//! T143 config / connect / exec / exit-code contract).

pub mod capabilities;
pub mod cli;
pub mod startup;

pub use capabilities::{
    parse_target, run_sftp_copy, ForwardSpec, ProxyChainSpec, ProxyHop, SftpSpec,
};
pub use cli::{
    Cli, CliConfig, CommandRunner, ConfigError, ExecResult, ExitCode, HostConfig, MockRunner,
    OutputMode,
};
pub use startup::{
    ActionablePrompt, DatabaseHealth, PromptSeverity, StartupConfig, StartupFlow, StartupOutcome,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "cli";
