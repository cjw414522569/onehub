//! Remote port forwarding engine (SSH `-R`) (T053).
//!
//! Implements the RFC 4254 §7.1 `tcpip-forward` global request codec plus a
//! [`RemoteForwarder`] state machine over an injectable [`RemoteForwardPeer`]:
//! dynamic port allocation (request port 0, server reports the allocated
//! port), rejection visibility (server `REQUEST_FAILURE`), and server-close
//! visibility. Incoming connections are piped to the local target through the
//! [`TargetConnector`] (shared with local forwarding). Real OpenSSH remote
//! forwarding integration is recorded as `blocked_environment` on this host
//! (no `ssh` binary); the wire format is exercised over `duplex` streams.

use std::future::Future;
use std::pin::Pin;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::local::{ChannelStream, TargetConnector};

/// `SSH_MSG_GLOBAL_REQUEST`.
pub const SSH_MSG_GLOBAL_REQUEST: u8 = 80;
/// `SSH_MSG_REQUEST_SUCCESS`.
pub const SSH_MSG_REQUEST_SUCCESS: u8 = 81;
/// `SSH_MSG_REQUEST_FAILURE`.
pub const SSH_MSG_REQUEST_FAILURE: u8 = 82;
/// The `tcpip-forward` request name.
pub const REQUEST_NAME_TCPIP_FORWARD: &str = "tcpip-forward";

/// Remote forwarding configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteForwardConfig {
    /// Bind address on the server side (e.g. `127.0.0.1` or `0.0.0.0`).
    pub listen_host: String,
    /// Bind port on the server side; `0` requests dynamic allocation.
    pub listen_port: u16,
    /// Local target host that incoming connections are piped to.
    pub target_host: String,
    /// Local target port.
    pub target_port: u16,
}

impl Default for RemoteForwardConfig {
    fn default() -> Self {
        Self {
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 0,
            target_host: String::new(),
            target_port: 22,
        }
    }
}

/// Server reply to a `tcpip-forward` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteForwardReply {
    /// Server accepted; for dynamic allocation the allocated port differs from
    /// the requested one.
    Accepted { allocated_port: u16 },
    /// Server refused the remote listen (`REQUEST_FAILURE`).
    Rejected { detail: String },
}

/// Event surfaced by the remote forwarder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteForwardEvent {
    /// The server closed the remote listener; forwarding is no longer active.
    ListenerClosed,
    /// The server opened an incoming connection that was piped to the local
    /// target.
    IncomingConnection { connection_id: u64 },
}

/// Remote forwarding error (no secret context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteForwardError {
    /// Underlying I/O failure.
    Io,
    /// The handshake timed out.
    Timeout,
    /// A protocol-level violation.
    Protocol(String),
}

impl RemoteForwardError {
    /// A protocol violation.
    pub fn protocol(detail: impl Into<String>) -> Self {
        Self::Protocol(detail.into())
    }
}

/// Encodes a `tcpip-forward` global request (RFC 4254 §7.1).
pub fn encode_tcpip_forward_request(host: &str, port: u16) -> Vec<u8> {
    let mut bytes = vec![SSH_MSG_GLOBAL_REQUEST];
    push_string(&mut bytes, REQUEST_NAME_TCPIP_FORWARD);
    bytes.push(0x01); // want_reply
    push_string(&mut bytes, host);
    bytes.extend_from_slice(&(u32::from(port)).to_be_bytes());
    bytes
}

/// Decodes a `tcpip-forward` global request: `(host, port)`.
pub fn decode_global_request(bytes: &[u8]) -> Result<(String, u16), RemoteForwardError> {
    if bytes.first() != Some(&SSH_MSG_GLOBAL_REQUEST) {
        return Err(RemoteForwardError::protocol("not a global request"));
    }
    let (name, rest) = take_string(&bytes[1..])?;
    if name != REQUEST_NAME_TCPIP_FORWARD {
        return Err(RemoteForwardError::protocol("not a tcpip-forward request"));
    }
    if rest.first() != Some(&0x01) {
        return Err(RemoteForwardError::protocol("want_reply must be set"));
    }
    let (host, rest) = take_string(&rest[1..])?;
    if rest.len() != 4 {
        return Err(RemoteForwardError::protocol("truncated port"));
    }
    let port = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
    if port > u32::from(u16::MAX) {
        return Err(RemoteForwardError::protocol("port out of range"));
    }
    Ok((host, port as u16))
}

/// Encodes a `SSH_MSG_REQUEST_SUCCESS` reply; `allocated_port` is included for
/// dynamic allocation.
pub fn encode_request_success(allocated_port: Option<u16>) -> Vec<u8> {
    let mut bytes = vec![SSH_MSG_REQUEST_SUCCESS];
    if let Some(port) = allocated_port {
        bytes.extend_from_slice(&(u32::from(port)).to_be_bytes());
    }
    bytes
}

/// Encodes a `SSH_MSG_REQUEST_FAILURE` reply.
pub fn encode_request_failure() -> Vec<u8> {
    vec![SSH_MSG_REQUEST_FAILURE]
}

/// Decodes a reply: `Ok(Some(port))` for success with a dynamic port,
/// `Ok(None)` for plain success, and a `Rejected` error for `REQUEST_FAILURE`.
pub fn decode_request_reply(bytes: &[u8]) -> Result<Option<u16>, RemoteForwardError> {
    match bytes.first() {
        Some(&SSH_MSG_REQUEST_SUCCESS) => {
            if bytes.len() == 1 {
                Ok(None)
            } else if bytes.len() == 5 {
                let port = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                if port > u32::from(u16::MAX) {
                    Err(RemoteForwardError::protocol("allocated port out of range"))
                } else {
                    Ok(Some(port as u16))
                }
            } else {
                Err(RemoteForwardError::protocol("invalid success reply length"))
            }
        }
        Some(&SSH_MSG_REQUEST_FAILURE) => Err(RemoteForwardError::protocol("server refused")),
        _ => Err(RemoteForwardError::protocol("unexpected reply message")),
    }
}

/// An injectable peer that performs the server side of the `tcpip-forward`
/// exchange (the real SSH engine wires in here).
pub trait RemoteForwardPeer: Send + Sync + 'static {
    /// Sends a `tcpip-forward` request and returns the server's reply.
    fn request_forward<'a>(
        &'a self,
        host: &'a str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteForwardReply, RemoteForwardError>> + Send + 'a>>;
}

/// A remote forwarder over an injectable peer.
pub struct RemoteForwarder<P> {
    peer: P,
    config: RemoteForwardConfig,
    allocated_port: Option<u16>,
    listening: bool,
    next_connection_id: u64,
}

impl<P: RemoteForwardPeer> RemoteForwarder<P> {
    /// Creates a forwarder.
    pub fn new(peer: P, config: RemoteForwardConfig) -> Self {
        Self {
            peer,
            config,
            allocated_port: None,
            listening: false,
            next_connection_id: 1,
        }
    }

    /// The port allocated by the server after a successful dynamic request.
    pub fn allocated_port(&self) -> Option<u16> {
        self.allocated_port
    }

    /// Whether the remote listener is active.
    pub fn is_listening(&self) -> bool {
        self.listening
    }

    /// The configuration.
    pub fn config(&self) -> &RemoteForwardConfig {
        &self.config
    }

    /// Sends the `tcpip-forward` request and applies the reply to the state.
    pub async fn establish(&mut self) -> Result<RemoteForwardReply, RemoteForwardError> {
        let reply = self
            .peer
            .request_forward(&self.config.listen_host, self.config.listen_port)
            .await?;
        match &reply {
            RemoteForwardReply::Accepted { allocated_port } => {
                self.allocated_port = Some(*allocated_port);
                self.listening = true;
            }
            RemoteForwardReply::Rejected { .. } => {
                self.allocated_port = None;
                self.listening = false;
            }
        }
        Ok(reply)
    }

    /// Pipes an incoming remote connection to the local target and returns an
    /// `IncomingConnection` event. The pipe runs in a background task.
    pub async fn pipe_incoming<C>(
        &mut self,
        connector: &C,
        remote: Box<dyn ChannelStream + Send>,
    ) -> Result<RemoteForwardEvent, RemoteForwardError>
    where
        C: TargetConnector,
    {
        let connection_id = self.next_connection_id;
        self.next_connection_id += 1;
        let connector = connector.clone();
        let host = self.config.target_host.clone();
        let port = self.config.target_port;
        tokio::spawn(async move {
            let mut remote = remote;
            if let Ok(mut target) = connector.connect(&host, port).await {
                let _ = tokio::io::copy_bidirectional(&mut remote, &mut target).await;
            }
        });
        Ok(RemoteForwardEvent::IncomingConnection { connection_id })
    }

    /// Marks the remote listener as closed by the server. Returns the
    /// `ListenerClosed` event so callers can surface it to the UI.
    pub fn mark_server_closed(&mut self) -> RemoteForwardEvent {
        self.listening = false;
        self.allocated_port = None;
        RemoteForwardEvent::ListenerClosed
    }
}

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    let length = value.len() as u32;
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn take_string(bytes: &[u8]) -> Result<(String, &[u8]), RemoteForwardError> {
    if bytes.len() < 4 {
        return Err(RemoteForwardError::protocol("truncated string length"));
    }
    let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < 4 + length {
        return Err(RemoteForwardError::protocol("truncated string"));
    }
    let value = std::str::from_utf8(&bytes[4..4 + length])
        .map_err(|_| RemoteForwardError::protocol("string is not UTF-8"))?
        .to_owned();
    Ok((value, &bytes[4 + length..]))
}

/// A wire peer that performs the real RFC 4254 §7.1 exchange over any
/// bidirectional stream (used by tests over `duplex`; the real SSH engine
/// plugs into the same trait).
pub struct WirePeer<S> {
    stream: tokio::sync::Mutex<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> WirePeer<S> {
    /// Wraps a stream.
    pub fn new(stream: S) -> Self {
        Self {
            stream: tokio::sync::Mutex::new(stream),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> RemoteForwardPeer for WirePeer<S> {
    fn request_forward<'a>(
        &'a self,
        host: &'a str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteForwardReply, RemoteForwardError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut stream = self.stream.lock().await;
            let request = encode_tcpip_forward_request(host, port);
            stream
                .write_all(&request)
                .await
                .map_err(|_| RemoteForwardError::Io)?;
            let mut reply = Vec::new();
            let mut first = [0u8; 1];
            stream
                .read_exact(&mut first)
                .await
                .map_err(|_| RemoteForwardError::Io)?;
            reply.push(first[0]);
            if first[0] == SSH_MSG_REQUEST_SUCCESS {
                // Success may carry a 4-byte allocated port.
                let mut port_bytes = [0u8; 4];
                stream
                    .read_exact(&mut port_bytes)
                    .await
                    .map_err(|_| RemoteForwardError::Io)?;
                reply.extend_from_slice(&port_bytes);
            }
            match decode_request_reply(&reply) {
                Ok(Some(port)) => Ok(RemoteForwardReply::Accepted {
                    allocated_port: port,
                }),
                Ok(None) => Ok(RemoteForwardReply::Accepted { allocated_port: 0 }),
                Err(_) => Ok(RemoteForwardReply::Rejected {
                    detail: "server refused".to_owned(),
                }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::local::TcpConnector;
    use crate::remote::{
        decode_global_request, decode_request_reply, encode_request_failure,
        encode_request_success, encode_tcpip_forward_request, RemoteForwardConfig,
        RemoteForwardEvent, RemoteForwardPeer, RemoteForwardReply, RemoteForwarder, WirePeer,
        REQUEST_NAME_TCPIP_FORWARD, SSH_MSG_GLOBAL_REQUEST, SSH_MSG_REQUEST_SUCCESS,
    };

    #[test]
    fn tcpip_forward_request_round_trip() {
        let bytes = encode_tcpip_forward_request("0.0.0.0", 0);
        assert_eq!(bytes[0], SSH_MSG_GLOBAL_REQUEST);
        let (host, port) = decode_global_request(&bytes).expect("decode");
        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 0);

        let bytes = encode_tcpip_forward_request("127.0.0.1", 2222);
        let (host, port) = decode_global_request(&bytes).expect("decode");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 2222);
    }

    #[test]
    fn request_reply_round_trip() {
        // Plain success.
        assert_eq!(
            decode_request_reply(&encode_request_success(None)).expect("ok"),
            None
        );
        // Success with dynamic port.
        assert_eq!(
            decode_request_reply(&encode_request_success(Some(30000))).expect("ok"),
            Some(30000)
        );
        // Failure -> protocol error (mapped to Rejected by the peer).
        assert!(decode_request_reply(&encode_request_failure()).is_err());
        // Malformed.
        assert!(decode_request_reply(&[0x99]).is_err());
        assert!(decode_request_reply(&[SSH_MSG_REQUEST_SUCCESS, 0x00]).is_err());
    }

    #[test]
    fn malformed_requests_are_rejected() {
        assert!(decode_global_request(&[]).is_err());
        assert!(decode_global_request(&[SSH_MSG_GLOBAL_REQUEST, 0, 0, 0, 3]).is_err());
        let wrong_name = {
            let mut bytes = vec![SSH_MSG_GLOBAL_REQUEST];
            let name = "cancel-tcpip-forward";
            bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(0x01);
            bytes.extend_from_slice(&(1u32).to_be_bytes());
            bytes.push(0);
            bytes
        };
        assert!(decode_global_request(&wrong_name).is_err());
    }

    /// A scripted server side that decodes the real request bytes and replies
    /// with success (dynamic port) or failure.
    async fn scripted_server(
        mut stream: tokio::io::DuplexStream,
        accept: bool,
        allocated_port: u16,
    ) -> (String, u16) {
        let mut head = [0u8; 1];
        stream.read_exact(&mut head).await.expect("msg type");
        assert_eq!(head[0], SSH_MSG_GLOBAL_REQUEST);
        let mut length = [0u8; 4];
        stream.read_exact(&mut length).await.expect("name length");
        let name_len = u32::from_be_bytes(length) as usize;
        let mut name = vec![0u8; name_len];
        stream.read_exact(&mut name).await.expect("name");
        assert_eq!(String::from_utf8_lossy(&name), REQUEST_NAME_TCPIP_FORWARD);
        let mut want = [0u8; 1];
        stream.read_exact(&mut want).await.expect("want reply");
        assert_eq!(want[0], 0x01);
        let mut host_len = [0u8; 4];
        stream.read_exact(&mut host_len).await.expect("host length");
        let host_len = u32::from_be_bytes(host_len) as usize;
        let mut host = vec![0u8; host_len];
        stream.read_exact(&mut host).await.expect("host");
        let mut port = [0u8; 4];
        stream.read_exact(&mut port).await.expect("port");
        let port = u32::from_be_bytes(port) as u16;
        if accept {
            stream
                .write_all(&encode_request_success(Some(allocated_port)))
                .await
                .expect("reply");
        } else {
            stream
                .write_all(&encode_request_failure())
                .await
                .expect("reply");
        }
        (String::from_utf8_lossy(&host).to_string(), port)
    }

    #[tokio::test]
    async fn dynamic_port_allocation_via_wire() {
        let (client, server) = duplex(4096);
        let server_handle = tokio::spawn(scripted_server(server, true, 30000));
        let config = RemoteForwardConfig {
            listen_host: "0.0.0.0".to_owned(),
            listen_port: 0,
            target_host: "127.0.0.1".to_owned(),
            target_port: 22,
        };
        let mut forwarder = RemoteForwarder::new(WirePeer::new(client), config);
        let reply = forwarder.establish().await.expect("establish");
        assert_eq!(
            reply,
            RemoteForwardReply::Accepted {
                allocated_port: 30000
            }
        );
        assert_eq!(forwarder.allocated_port(), Some(30000));
        assert!(forwarder.is_listening());
        let (host, port) = server_handle.await.expect("server joined");
        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 0);
    }

    #[tokio::test]
    async fn rejection_is_visible_via_wire() {
        let (client, server) = duplex(4096);
        let server_handle = tokio::spawn(scripted_server(server, false, 0));
        let config = RemoteForwardConfig {
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 8080,
            target_host: "127.0.0.1".to_owned(),
            target_port: 22,
        };
        let mut forwarder = RemoteForwarder::new(WirePeer::new(client), config);
        let reply = forwarder
            .establish()
            .await
            .expect("establish returns reply");
        assert!(matches!(reply, RemoteForwardReply::Rejected { .. }));
        assert!(!forwarder.is_listening());
        assert_eq!(forwarder.allocated_port(), None);
        let (host, port) = server_handle.await.expect("server joined");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8080);
    }

    #[tokio::test]
    async fn server_close_is_visible() {
        let (client, server) = duplex(4096);
        let _server_handle = tokio::spawn(scripted_server(server, true, 2222));
        let config = RemoteForwardConfig::default();
        let mut forwarder = RemoteForwarder::new(WirePeer::new(client), config);
        forwarder.establish().await.expect("establish");
        assert!(forwarder.is_listening());

        let event = forwarder.mark_server_closed();
        assert_eq!(event, RemoteForwardEvent::ListenerClosed);
        assert!(!forwarder.is_listening());
        assert_eq!(forwarder.allocated_port(), None);
    }

    async fn echo_server() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    loop {
                        match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if stream.write_all(&buffer[..n]).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
        });
        address
    }

    #[tokio::test]
    async fn incoming_connection_is_piped_to_local_target() {
        let echo = echo_server().await;
        let (client, server) = duplex(4096);
        let _server_handle = tokio::spawn(scripted_server(server, true, 2222));
        let config = RemoteForwardConfig {
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 2222,
            target_host: echo.ip().to_string(),
            target_port: echo.port(),
        };
        let mut forwarder = RemoteForwarder::new(WirePeer::new(client), config);
        forwarder.establish().await.expect("establish");

        // Simulate the server opening a forwarded-tcpip channel: hand the
        // forwarder one half of a fresh duplex; the test holds the other.
        let (remote_side, test_side) = duplex(4096);
        let event = forwarder
            .pipe_incoming(&TcpConnector, Box::new(remote_side))
            .await
            .expect("pipe incoming");
        assert!(matches!(
            event,
            RemoteForwardEvent::IncomingConnection { connection_id: 1 }
        ));

        // The echo round trip flows through the forwarder's pipe task.
        let mut test_side = test_side;
        test_side.write_all(b"ping").await.expect("write");
        let mut echoed = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(1), test_side.read_exact(&mut echoed))
            .await
            .expect("read echo within 1s")
            .expect("read exact");
        assert_eq!(&echoed, b"ping");
    }

    #[tokio::test]
    async fn wire_peer_maps_failure_reply_to_rejected() {
        let (client, server) = duplex(4096);
        let _server_handle = tokio::spawn(scripted_server(server, false, 0));
        let peer = WirePeer::new(client);
        let reply = peer.request_forward("127.0.0.1", 0).await.expect("reply");
        assert!(matches!(reply, RemoteForwardReply::Rejected { .. }));
    }
}
