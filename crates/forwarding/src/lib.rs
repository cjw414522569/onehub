#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # forwarding
//!
//! Local / remote / dynamic port forwarding engines (T052-T054). The local
//! forwarder binds a listener, opens a channel to the remote target through an
//! injectable [`TargetConnector`], and pipes bytes bidirectionally with a
//! concurrent-connection cap and graceful shutdown.

pub mod local;
pub mod remote;

pub use local::{
    BindScope, ChannelStream, ForwardError, LocalForwardConfig, LocalForwarder, TargetConnector,
    TcpConnector,
};
pub use remote::{
    decode_global_request, decode_request_reply, encode_request_failure, encode_request_success,
    encode_tcpip_forward_request, RemoteForwardConfig, RemoteForwardError, RemoteForwardEvent,
    RemoteForwardPeer, RemoteForwardReply, RemoteForwarder, WirePeer, REQUEST_NAME_TCPIP_FORWARD,
    SSH_MSG_GLOBAL_REQUEST, SSH_MSG_REQUEST_FAILURE, SSH_MSG_REQUEST_SUCCESS,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "forwarding";
