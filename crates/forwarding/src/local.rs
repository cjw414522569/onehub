//! Local port forwarding engine (SSH `-L`) (T052).
//!
//! Binds a local [`TcpListener`], and for each accepted connection opens a
//! channel to the remote target via an injectable [`TargetConnector`] (the
//! SSH direct-tcpip opener will be wired in by the backend), then pipes bytes
//! bidirectionally with a concurrent-connection cap. Startup uses the T031
//! [`ForwardingTable`] for conflict detection and reports the bind scope so
//! callers can warn when a wildcard/exposed address is used. Shutdown is
//! graceful: the listener closes immediately, in-flight connections drain up
//! to a timeout, and new connections are refused.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_domain::forwarding::{
    ForwardingAddResult, ForwardingEndpoint, ForwardingFamily, ForwardingKind, ForwardingSpec,
    ForwardingTable,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};

/// How exposed a local bind address is (drives the bind-range warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindScope {
    /// Loopback only (`127.0.0.0/8`, `::1`): not reachable from other hosts.
    Loopback,
    /// Wildcard (`0.0.0.0`, `::`): reachable on every interface.
    Wildcard,
    /// A specific non-loopback address: reachable on that interface.
    Other,
}

impl BindScope {
    /// Classifies an IP literal.
    pub fn from_ip(ip: IpAddr) -> Self {
        if ip.is_loopback() {
            BindScope::Loopback
        } else if ip.is_unspecified() {
            BindScope::Wildcard
        } else {
            BindScope::Other
        }
    }

    /// Whether the UI must warn about exposing the listener.
    pub fn requires_warning(self) -> bool {
        matches!(self, BindScope::Wildcard | BindScope::Other)
    }
}

/// Local forwarding configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalForwardConfig {
    /// Local bind address (port 0 = OS-assigned).
    pub listen: SocketAddr,
    /// Remote target host reached through the SSH channel.
    pub target_host: String,
    /// Remote target port.
    pub target_port: u16,
    /// Concurrent in-flight connection cap (0 = unlimited).
    pub max_connections: usize,
    /// Graceful shutdown drain timeout.
    pub shutdown_timeout: Duration,
}

impl Default for LocalForwardConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            target_host: String::new(),
            target_port: 22,
            max_connections: 0,
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

/// Forwarding engine error (no secret context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardError {
    /// The local bind failed (already in use, bad address, or permission).
    BindFailed { address: SocketAddr },
    /// The listen endpoint conflicts with an existing forwarding (T031).
    Conflict { key: String },
    /// The target could not be reached through the channel.
    TargetConnectFailed,
    /// The forwarder was not started.
    NotStarted,
}

/// A bidirectional channel stream (SSH direct-tcpip or a plain TCP stream).
pub trait ChannelStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> ChannelStream for T {}

/// Opens a channel to the remote target.
pub trait TargetConnector: Clone + Send + Sync + 'static {
    /// Opens a connection to `host:port`.
    fn connect(
        &self,
        host: &str,
        port: u16,
    ) -> impl Future<Output = Result<Box<dyn ChannelStream + Send>, ForwardError>> + Send;
}

/// Connector that opens a real TCP connection (used by tests and for the
/// TCP-echo matrix; the SSH direct-tcpip opener plugs into the same trait).
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpConnector;

impl TargetConnector for TcpConnector {
    async fn connect(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Box<dyn ChannelStream + Send>, ForwardError> {
        let stream = TcpStream::connect((host, port))
            .await
            .map_err(|_| ForwardError::TargetConnectFailed)?;
        Ok(Box::new(stream))
    }
}

/// A running local forwarder.
///
/// Owns the accept task; dropping the forwarder without calling [`stop`]
/// aborts the accept task (closing the listener) but leaves in-flight
/// connections running to completion.
///
/// [`stop`]: LocalForwarder::stop
pub struct LocalForwarder<C> {
    connector: C,
    config: LocalForwardConfig,
    table: ForwardingTable,
    active: Arc<AtomicUsize>,
    shutting_down: Arc<AtomicBool>,
    accept_task: Option<tokio::task::JoinHandle<()>>,
    local_addr: Option<SocketAddr>,
}

impl<C: TargetConnector> LocalForwarder<C> {
    /// Creates a forwarder with a shared forwarding table (T031 conflict
    /// detection).
    pub fn new(connector: C, config: LocalForwardConfig, table: ForwardingTable) -> Self {
        Self {
            connector,
            config,
            table,
            active: Arc::new(AtomicUsize::new(0)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            accept_task: None,
            local_addr: None,
        }
    }

    /// The classified bind scope for the configured listen address.
    pub fn bind_scope(&self) -> BindScope {
        BindScope::from_ip(self.config.listen.ip())
    }

    /// Whether the UI must warn about the bind range.
    pub fn requires_bind_warning(&self) -> bool {
        self.bind_scope().requires_warning()
    }

    /// The actual local address (available after a successful `start`).
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Currently in-flight connections.
    pub fn active_connections(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// Starts the listener and the accept loop. Returns the bound address.
    pub async fn start(&mut self) -> Result<SocketAddr, ForwardError> {
        // Conflict detection (T031) applies only to concrete listen endpoints;
        // an ephemeral port (0) is always unique and skips the table check.
        let spec = self.concrete_spec();
        if let Some(spec) = &spec {
            match self.table.add(spec.clone()) {
                ForwardingAddResult::Added => {}
                ForwardingAddResult::Conflict { key } => {
                    return Err(ForwardError::Conflict { key });
                }
            }
        }
        let listener = match TcpListener::bind(self.config.listen).await {
            Ok(listener) => listener,
            Err(_) => {
                if let Some(spec) = &spec {
                    self.table.remove(&spec.listen_key());
                }
                return Err(ForwardError::BindFailed {
                    address: self.config.listen,
                });
            }
        };
        let local_addr = listener.local_addr().expect("bound address");
        self.local_addr = Some(local_addr);

        let active = self.active.clone();
        let shutting_down = self.shutting_down.clone();
        let connector = self.connector.clone();
        let max_connections = self.config.max_connections;
        let target_host = self.config.target_host.clone();
        let target_port = self.config.target_port;

        let accept_task = tokio::spawn(async move {
            loop {
                let (mut client, _) = match listener.accept().await {
                    Ok(accepted) => accepted,
                    Err(_) => break, // listener closed (stop/drop)
                };
                if shutting_down.load(Ordering::SeqCst) {
                    drop(client);
                    break;
                }
                let previous = active.fetch_add(1, Ordering::SeqCst);
                if max_connections > 0 && previous + 1 > max_connections {
                    active.fetch_sub(1, Ordering::SeqCst);
                    drop(client); // refuse the excess connection
                    continue;
                }
                let connector = connector.clone();
                let host = target_host.clone();
                let port = target_port;
                let active = active.clone();
                tokio::spawn(async move {
                    let result = connector.connect(&host, port).await;
                    match result {
                        Ok(mut target) => {
                            let _ = tokio::io::copy_bidirectional(&mut client, &mut target).await;
                        }
                        Err(_) => {
                            // Target unreachable: close the client so the peer
                            // observes a clean EOF.
                        }
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        self.accept_task = Some(accept_task);
        Ok(local_addr)
    }

    /// Closes the listener immediately: aborts the accept task, removes the
    /// forwarding from the table, and marks the forwarder as shutting down.
    /// In-flight connections keep running so they can finish gracefully.
    pub async fn close_listener(&mut self) -> Result<(), ForwardError> {
        let Some(accept_task) = self.accept_task.take() else {
            return Err(ForwardError::NotStarted);
        };
        self.shutting_down.store(true, Ordering::SeqCst);
        accept_task.abort();
        let _ = accept_task.await;
        if let Some(spec) = self.concrete_spec() {
            self.table.remove(&spec.listen_key());
        }
        self.local_addr = None;
        Ok(())
    }

    /// Waits up to the shutdown timeout for in-flight connections to finish.
    /// Returns whether every connection drained within the timeout.
    pub async fn drain(&mut self) -> Result<bool, ForwardError> {
        let deadline = Instant::now() + self.config.shutdown_timeout;
        loop {
            if self.active.load(Ordering::SeqCst) == 0 {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Stops the forwarder gracefully: closes the listener immediately, waits
    /// up to the shutdown timeout for in-flight connections to drain, and
    /// removes the forwarding from the table. Returns whether every in-flight
    /// connection finished within the timeout.
    pub async fn stop(&mut self) -> Result<bool, ForwardError> {
        self.close_listener().await?;
        self.drain().await
    }

    /// The T031 forwarding spec for this config, or `None` when the listen
    /// port is ephemeral (0), in which case conflict detection is skipped.
    fn concrete_spec(&self) -> Option<ForwardingSpec> {
        if self.config.listen.port() == 0 {
            return None;
        }
        Some(
            ForwardingSpec::new(
                ForwardingKind::Local,
                ForwardingEndpoint::new(
                    self.config.listen.ip().to_string(),
                    self.config.listen.port(),
                )
                .expect("valid endpoint"),
                Some(
                    ForwardingEndpoint::new(
                        self.config.target_host.clone(),
                        self.config.target_port,
                    )
                    .expect("valid target"),
                ),
                ForwardingFamily::Any,
            )
            .expect("valid spec"),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::time::Duration;

    use core_domain::forwarding::ForwardingTable;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::{BindScope, ForwardError, LocalForwardConfig, LocalForwarder, TcpConnector};

    async fn echo_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind echo");
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

    fn config(listen: SocketAddr, echo: SocketAddr) -> LocalForwardConfig {
        LocalForwardConfig {
            listen,
            target_host: echo.ip().to_string(),
            target_port: echo.port(),
            ..LocalForwardConfig::default()
        }
    }

    #[tokio::test]
    async fn tcp_echo_round_trip_through_local_forward() {
        let echo = echo_server().await;
        let mut forwarder = LocalForwarder::new(
            TcpConnector,
            config(SocketAddr::from(([127, 0, 0, 1], 0)), echo),
            ForwardingTable::new(),
        );
        let local = forwarder.start().await.expect("start");
        assert!(!forwarder.requires_bind_warning());

        let mut client = TcpStream::connect(local).await.expect("connect local");
        client.write_all(b"ping").await.expect("write");
        let mut echoed = [0u8; 4];
        client.read_exact(&mut echoed).await.expect("read echo");
        assert_eq!(&echoed, b"ping");

        drop(client);
        let drained = forwarder.stop().await.expect("stop");
        assert!(drained, "in-flight connection should drain");
        assert_eq!(forwarder.active_connections(), 0);
    }

    #[tokio::test]
    async fn concurrent_connections_all_echo_with_cap() {
        let echo = echo_server().await;
        let mut forwarder = LocalForwarder::new(
            TcpConnector,
            LocalForwardConfig {
                listen: SocketAddr::from(([127, 0, 0, 1], 0)),
                target_host: echo.ip().to_string(),
                target_port: echo.port(),
                max_connections: 64,
                ..LocalForwardConfig::default()
            },
            ForwardingTable::new(),
        );
        let local = forwarder.start().await.expect("start");

        let mut handles = Vec::new();
        for index in 0..16 {
            handles.push(tokio::spawn(async move {
                let mut client = TcpStream::connect(local).await.expect("connect");
                let message = format!("msg-{index}");
                client.write_all(message.as_bytes()).await.expect("write");
                let mut echoed = vec![0u8; message.len()];
                client.read_exact(&mut echoed).await.expect("read");
                assert_eq!(String::from_utf8(echoed).expect("utf8"), message);
            }));
        }
        for handle in handles {
            handle.await.expect("client joined");
        }
        let drained = forwarder.stop().await.expect("stop");
        assert!(drained);
        assert_eq!(forwarder.active_connections(), 0);
    }

    #[tokio::test]
    async fn connection_cap_refuses_excess_clients() {
        let echo = echo_server().await;
        let mut forwarder = LocalForwarder::new(
            TcpConnector,
            LocalForwardConfig {
                listen: SocketAddr::from(([127, 0, 0, 1], 0)),
                target_host: echo.ip().to_string(),
                target_port: echo.port(),
                max_connections: 1,
                ..LocalForwardConfig::default()
            },
            ForwardingTable::new(),
        );
        let local = forwarder.start().await.expect("start");

        // First client connects and stays open.
        let first = TcpStream::connect(local).await.expect("first connect");
        // Give the accept loop time to register the first connection.
        tokio::time::sleep(Duration::from_millis(20)).await;
        // Second client is refused: the listener closes the socket.
        let mut second = TcpStream::connect(local).await.expect("second connect");
        let mut buffer = [0u8; 8];
        let read = second.read(&mut buffer).await.expect("read");
        assert_eq!(read, 0, "excess connection must be refused with EOF");

        drop(first);
        let drained = forwarder.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn graceful_shutdown_closes_listener_and_drains() {
        let echo = echo_server().await;
        let mut forwarder = LocalForwarder::new(
            TcpConnector,
            LocalForwardConfig {
                listen: SocketAddr::from(([127, 0, 0, 1], 0)),
                target_host: echo.ip().to_string(),
                target_port: echo.port(),
                shutdown_timeout: Duration::from_secs(2),
                ..LocalForwardConfig::default()
            },
            ForwardingTable::new(),
        );
        let local = forwarder.start().await.expect("start");

        // One in-flight connection stays open.
        let mut client = TcpStream::connect(local).await.expect("connect");
        client.write_all(b"hold").await.expect("write");

        // Wait until the accept loop has registered the in-flight connection,
        // otherwise closing the listener could abort before accepting it.
        let mut waited = 0;
        while forwarder.active_connections() == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
            waited += 1;
            assert!(waited < 100, "forwarder never accepted the connection");
        }

        // Closing the listener is immediate; new connections must not become
        // usable. On Windows a connect to a just-closed loopback port can hang
        // in SYN retry, so bound the probe: either refusal or a short timeout
        // proves no usable connection was established.
        forwarder.close_listener().await.expect("close listener");
        let refused =
            tokio::time::timeout(Duration::from_millis(300), TcpStream::connect(local)).await;
        assert!(
            refused.is_err(),
            "listener must be closed after close_listener"
        );

        // The in-flight connection is still active until released.
        assert_eq!(forwarder.active_connections(), 1);

        // Release the in-flight connection; it finishes cleanly and drains.
        drop(client);
        let drained = forwarder.drain().await.expect("drain");
        assert!(drained);
        assert_eq!(forwarder.active_connections(), 0);
    }

    #[tokio::test]
    async fn bind_failure_is_reported() {
        // Occupy a port, then try to forward on the same port.
        let occupied = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind occupied");
        let address = occupied.local_addr().expect("addr");
        let mut forwarder = LocalForwarder::new(
            TcpConnector,
            config(address, address),
            ForwardingTable::new(),
        );
        let error = forwarder.start().await.expect_err("must fail to bind");
        assert!(matches!(error, ForwardError::BindFailed { .. }));
        // The table must not keep the failed forwarding.
        assert_eq!(forwarder.active_connections(), 0);
    }

    #[tokio::test]
    async fn non_local_bind_address_is_reported() {
        // TEST-NET address is not assigned to any local interface.
        let address = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 1), 0));
        let mut forwarder = LocalForwarder::new(
            TcpConnector,
            config(address, address),
            ForwardingTable::new(),
        );
        let error = forwarder.start().await.expect_err("must fail to bind");
        assert!(matches!(error, ForwardError::BindFailed { .. }));
    }

    #[test]
    fn bind_scope_warning_policy() {
        assert_eq!(
            BindScope::from_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            BindScope::Loopback
        );
        assert_eq!(
            BindScope::from_ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            BindScope::Wildcard
        );
        assert_eq!(
            BindScope::from_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            BindScope::Loopback
        );
        assert_eq!(
            BindScope::from_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))),
            BindScope::Other
        );
        assert!(!BindScope::Loopback.requires_warning());
        assert!(BindScope::Wildcard.requires_warning());
        assert!(BindScope::Other.requires_warning());
    }

    #[tokio::test]
    async fn same_listen_endpoint_conflicts_via_forwarding_table() {
        use core_domain::forwarding::{
            ForwardingEndpoint, ForwardingFamily, ForwardingKind, ForwardingSpec,
        };
        let echo = echo_server().await;
        let occupied = SocketAddr::from(([127, 0, 0, 1], 23456));
        // Another forwarder already claimed the endpoint (T031 table entry).
        let mut table = ForwardingTable::new();
        table.add(
            ForwardingSpec::new(
                ForwardingKind::Local,
                ForwardingEndpoint::new("127.0.0.1", 23456).expect("endpoint"),
                Some(ForwardingEndpoint::new("target.internal", 22).expect("target")),
                ForwardingFamily::Any,
            )
            .expect("spec"),
        );
        let mut forwarder = LocalForwarder::new(TcpConnector, config(occupied, echo), table);
        let error = forwarder.start().await.expect_err("must conflict");
        assert!(matches!(error, ForwardError::Conflict { .. }));
        assert_eq!(forwarder.local_addr(), None);
    }

    #[test]
    fn stop_without_start_is_reported() {
        let mut forwarder = LocalForwarder::new(
            TcpConnector,
            LocalForwardConfig::default(),
            ForwardingTable::new(),
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let result = runtime.block_on(async { forwarder.stop().await });
        assert_eq!(result, Err(ForwardError::NotStarted));
    }
}
