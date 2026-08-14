#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # forwarding
//!
//! Local / remote / dynamic port forwarding engines (T052-T054). The local
//! forwarder binds a listener, opens a channel to the remote target through an
//! injectable [`TargetConnector`], and pipes bytes bidirectionally with a
//! concurrent-connection cap and graceful shutdown.

pub mod local;

pub use local::{
    BindScope, ChannelStream, ForwardError, LocalForwardConfig, LocalForwarder, TargetConnector,
    TcpConnector,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "forwarding";
