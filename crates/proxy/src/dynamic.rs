//! Dynamic SOCKS5 forwarding server (SSH `-D`) (T054).
//!
//! Listens locally and behaves as a SOCKS5 server (reusing the T051 codec):
//! it negotiates the method (optionally RFC 1929 auth), reads a CONNECT
//! request for an IPv4 / IPv6 / domain target, applies an access policy, and
//! forwards the connection through an injectable [`TargetConnector`] (the SSH
//! direct-tcpip channel opener). CONNECT, address-family handling and access
//! policy are the acceptance surface.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use forwarding::{ForwardError, TargetConnector};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::socks5::{
    decode_auth_request, decode_connect_request, encode_reply, ProxyTarget, METHOD_NO_ACCEPTABLE,
    METHOD_NO_AUTH, METHOD_USER_PASS, REP_CONNECTION_REFUSED, REP_SUCCESS, SOCKS5_VERSION,
};

/// Reply code for "connection not allowed by ruleset".
pub const REP_NOT_ALLOWED: u8 = 0x02;

/// Access policy for dynamic SOCKS forwarding (anti-proxy-abuse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessPolicy {
    /// Allow connections to any destination.
    AllowAll,
    /// Allow only loopback destinations.
    LoopbackOnly,
    /// Allow only the listed hosts.
    Allowlist(Vec<String>),
}

impl AccessPolicy {
    /// Whether `target` is permitted.
    pub fn allows(&self, target: &ProxyTarget) -> bool {
        match self {
            AccessPolicy::AllowAll => true,
            AccessPolicy::LoopbackOnly => match target {
                ProxyTarget::Ip(ip) => ip.is_loopback(),
                ProxyTarget::Hostname(host) => {
                    matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1")
                }
            },
            AccessPolicy::Allowlist(hosts) => hosts.iter().any(|host| target.host_str() == *host),
        }
    }
}

/// Dynamic SOCKS5 forwarding server configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicSocksConfig {
    /// Local listen address (port 0 = OS-assigned).
    pub listen: SocketAddr,
    /// Access policy.
    pub access: AccessPolicy,
    /// Optional RFC 1929 username; when `Some` the server requires auth.
    pub username: Option<String>,
    /// Optional RFC 1929 password (never logged).
    pub password: Option<String>,
    /// Concurrent connection cap (0 = unlimited).
    pub max_connections: usize,
    /// Handshake timeout.
    pub timeout: Duration,
}

impl Default for DynamicSocksConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            access: AccessPolicy::AllowAll,
            username: None,
            password: None,
            max_connections: 0,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Dynamic SOCKS server error (no secret context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicSocksError {
    /// The local bind failed.
    BindFailed { address: SocketAddr },
    /// The server was not started.
    NotStarted,
}

/// A running dynamic SOCKS5 server.
pub struct DynamicSocksServer<C> {
    connector: C,
    config: DynamicSocksConfig,
    active: Arc<AtomicUsize>,
    accept_task: Option<tokio::task::JoinHandle<()>>,
    local_addr: Option<SocketAddr>,
}

impl<C: TargetConnector> DynamicSocksServer<C> {
    /// Creates a server.
    pub fn new(connector: C, config: DynamicSocksConfig) -> Self {
        Self {
            connector,
            config,
            active: Arc::new(AtomicUsize::new(0)),
            accept_task: None,
            local_addr: None,
        }
    }

    /// Currently in-flight connections.
    pub fn active_connections(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// The bound local address after a successful `start`.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Starts the listener and accept loop; returns the bound address.
    pub async fn start(&mut self) -> Result<SocketAddr, DynamicSocksError> {
        let listener = match TcpListener::bind(self.config.listen).await {
            Ok(listener) => listener,
            Err(_) => {
                return Err(DynamicSocksError::BindFailed {
                    address: self.config.listen,
                });
            }
        };
        let local_addr = listener.local_addr().expect("bound address");
        self.local_addr = Some(local_addr);

        let active = self.active.clone();
        let connector = self.connector.clone();
        let config = self.config.clone();
        let accept_task = tokio::spawn(async move {
            loop {
                let (client, _) = match listener.accept().await {
                    Ok(accepted) => accepted,
                    Err(_) => break,
                };
                let previous = active.fetch_add(1, Ordering::SeqCst);
                if config.max_connections > 0 && previous + 1 > config.max_connections {
                    active.fetch_sub(1, Ordering::SeqCst);
                    drop(client);
                    continue;
                }
                let connector = connector.clone();
                let config = config.clone();
                let active = active.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(client, &connector, &config).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        self.accept_task = Some(accept_task);
        Ok(local_addr)
    }

    /// Stops the server: closes the listener immediately and waits for
    /// in-flight connections to drain (bounded by the config timeout).
    pub async fn stop(&mut self) -> Result<bool, DynamicSocksError> {
        let Some(accept_task) = self.accept_task.take() else {
            return Err(DynamicSocksError::NotStarted);
        };
        accept_task.abort();
        let _ = accept_task.await;
        self.local_addr = None;
        let deadline = std::time::Instant::now() + self.config.timeout;
        loop {
            if self.active.load(Ordering::SeqCst) == 0 {
                return Ok(true);
            }
            if std::time::Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

/// Handles one SOCKS5 client connection. The handshake is bounded by the
/// config timeout; the established tunnel pipes without a time limit.
async fn handle_connection<C>(
    mut client: impl AsyncRead + AsyncWrite + Unpin,
    connector: &C,
    config: &DynamicSocksConfig,
) -> Result<(), DynamicSocksError>
where
    C: TargetConnector,
{
    let handshake = async {
        // 1. Greeting.
        let greeting = match read_greeting(&mut client).await {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        if greeting.first() != Some(&SOCKS5_VERSION) {
            return Ok(None);
        }
        let methods = &greeting[2..];
        if config.username.is_some() {
            if !methods.contains(&METHOD_USER_PASS) {
                let _ = client
                    .write_all(&[SOCKS5_VERSION, METHOD_NO_ACCEPTABLE])
                    .await;
                return Ok(None);
            }
            let _ = client.write_all(&[SOCKS5_VERSION, METHOD_USER_PASS]).await;
            let auth = match read_auth(&mut client).await {
                Ok(bytes) => bytes,
                Err(_) => return Ok(None),
            };
            let (username, password) = match decode_auth_request(&auth) {
                Ok(pair) => pair,
                Err(_) => return Ok(None),
            };
            let ok = config.username.as_deref() == Some(username.as_str())
                && config.password.as_deref() == Some(password.as_str());
            let _ = client
                .write_all(&[0x01, if ok { 0x00 } else { 0x01 }])
                .await;
            if !ok {
                return Ok(None);
            }
        } else {
            let _ = client.write_all(&[SOCKS5_VERSION, METHOD_NO_AUTH]).await;
        }

        // 2. CONNECT request.
        let request = match read_connect_request(&mut client).await {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        let (target, port) = match decode_connect_request(&request) {
            Ok(pair) => pair,
            Err(_) => return Ok(None),
        };

        // 3. Access policy.
        if !config.access.allows(&target) {
            let _ = client
                .write_all(&encode_reply(
                    REP_NOT_ALLOWED,
                    SocketAddr::from(([0, 0, 0, 0], 0)),
                ))
                .await;
            return Ok(None);
        }

        // 4. Open the outbound connection (SSH direct-tcpip channel).
        let outbound = match connector.connect(&target.host_str(), port).await {
            Ok(stream) => stream,
            Err(ForwardError::TargetConnectFailed) => {
                let _ = client
                    .write_all(&encode_reply(
                        REP_CONNECTION_REFUSED,
                        SocketAddr::from(([0, 0, 0, 0], 0)),
                    ))
                    .await;
                return Ok(None);
            }
            Err(_) => return Ok(None),
        };

        // 5. Success reply.
        let bind = SocketAddr::from(([127, 0, 0, 1], 0));
        if client
            .write_all(&encode_reply(REP_SUCCESS, bind))
            .await
            .is_err()
        {
            return Ok(None);
        }
        Ok::<_, DynamicSocksError>(Some(outbound))
    };

    let outbound = match tokio::time::timeout(config.timeout, handshake).await {
        Ok(Ok(Some(outbound))) => outbound,
        _ => return Ok(()),
    };

    // Tunnel without a time limit.
    let mut outbound = outbound;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut outbound).await;
    Ok(())
}

/// Reads the greeting: `VER, NMETHODS, METHODS...`.
async fn read_greeting(stream: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await?;
    let mut methods = vec![0u8; head[1] as usize];
    stream.read_exact(&mut methods).await?;
    let mut bytes = vec![head[0], head[1]];
    bytes.extend_from_slice(&methods);
    Ok(bytes)
}

/// Reads an RFC 1929 auth request.
async fn read_auth(stream: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut version = [0u8; 1];
    stream.read_exact(&mut version).await?;
    let mut ulen = [0u8; 1];
    stream.read_exact(&mut ulen).await?;
    let mut username = vec![0u8; ulen[0] as usize];
    stream.read_exact(&mut username).await?;
    let mut plen = [0u8; 1];
    stream.read_exact(&mut plen).await?;
    let mut password = vec![0u8; plen[0] as usize];
    stream.read_exact(&mut password).await?;
    let mut bytes = vec![version[0], ulen[0]];
    bytes.extend_from_slice(&username);
    bytes.push(plen[0]);
    bytes.extend_from_slice(&password);
    Ok(bytes)
}

/// Reads a CONNECT request: `VER, CMD, RSV, ATYP, ...`.
async fn read_connect_request(stream: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    let mut bytes = vec![head[0], head[1], head[2], head[3]];
    let address_length = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut length = [0u8; 1];
            stream.read_exact(&mut length).await?;
            bytes.push(length[0]);
            length[0] as usize
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported ATYP",
            ));
        }
    };
    let mut rest = vec![0u8; address_length + 2];
    stream.read_exact(&mut rest).await?;
    bytes.extend_from_slice(&rest);
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;

    use forwarding::{ChannelStream, ForwardError, TargetConnector};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::{
        AccessPolicy, DynamicSocksConfig, DynamicSocksError, DynamicSocksServer, REP_NOT_ALLOWED,
    };
    use crate::socks5::{socks5_connect, ProxyTarget, Socks5Config};
    use crate::ProxyErrorKind;

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

    /// Maps hostnames (and v6 literals) to a concrete echo address; unmapped
    /// hosts connect literally so IPv4 literals still work.
    #[derive(Clone, Default)]
    struct MapConnector {
        entries: Arc<HashMap<String, SocketAddr>>,
    }

    impl MapConnector {
        fn new(entries: &[(&str, SocketAddr)]) -> Self {
            Self {
                entries: Arc::new(
                    entries
                        .iter()
                        .map(|(host, addr)| ((*host).to_owned(), *addr))
                        .collect(),
                ),
            }
        }
    }

    impl TargetConnector for MapConnector {
        async fn connect(
            &self,
            host: &str,
            port: u16,
        ) -> Result<Box<dyn ChannelStream + Send>, ForwardError> {
            let stream = match self.entries.get(host) {
                Some(addr) => TcpStream::connect(*addr).await,
                None => TcpStream::connect((host, port)).await,
            }
            .map_err(|_| ForwardError::TargetConnectFailed)?;
            Ok(Box::new(stream))
        }
    }

    fn config(access: AccessPolicy) -> DynamicSocksConfig {
        DynamicSocksConfig {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            access,
            ..DynamicSocksConfig::default()
        }
    }

    async fn start_server(
        connector: MapConnector,
        config: DynamicSocksConfig,
    ) -> (DynamicSocksServer<MapConnector>, SocketAddr) {
        let mut server = DynamicSocksServer::new(connector, config);
        let addr = server.start().await.expect("start");
        (server, addr)
    }

    /// Opens a SOCKS5 session through the dynamic server and does an echo
    /// round trip; returns the echoed bytes.
    async fn echo_round_trip(
        server_addr: SocketAddr,
        target: &ProxyTarget,
        port: u16,
        config: &Socks5Config,
    ) -> Result<Vec<u8>, crate::ProxyError> {
        let mut stream = TcpStream::connect(server_addr)
            .await
            .expect("connect to dynamic server");
        socks5_connect(&mut stream, target, port, config).await?;
        stream.write_all(b"ping").await.expect("write");
        let mut echoed = [0u8; 4];
        stream.read_exact(&mut echoed).await.expect("read echo");
        Ok(echoed.to_vec())
    }

    #[tokio::test]
    async fn connect_ipv4_target_echoes() {
        let echo = echo_server().await;
        let (mut server, addr) =
            start_server(MapConnector::default(), config(AccessPolicy::AllowAll)).await;
        let echoed = echo_round_trip(
            addr,
            &ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            echo.port(),
            &Socks5Config::default(),
        )
        .await
        .expect("echo via dynamic server");
        assert_eq!(echoed, b"ping");
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn connect_domain_target_echoes() {
        let echo = echo_server().await;
        let connector = MapConnector::new(&[("example.test", echo)]);
        let (mut server, addr) = start_server(connector, config(AccessPolicy::AllowAll)).await;
        let target = ProxyTarget::Hostname("example.test".to_owned());
        let echoed = echo_round_trip(addr, &target, 443, &Socks5Config::default())
            .await
            .expect("domain echo");
        assert_eq!(echoed, b"ping");
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn connect_ipv6_literal_target_echoes() {
        let echo = echo_server().await;
        let connector = MapConnector::new(&[("::1", echo)]);
        let (mut server, addr) = start_server(connector, config(AccessPolicy::AllowAll)).await;
        let target = ProxyTarget::Ip(IpAddr::V6("::1".parse().expect("v6")));
        let echoed = echo_round_trip(addr, &target, 22, &Socks5Config::default())
            .await
            .expect("ipv6 echo");
        assert_eq!(echoed, b"ping");
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn access_allowlist_permits_and_denies() {
        let echo = echo_server().await;
        let connector = MapConnector::new(&[("allowed.test", echo), ("denied.test", echo)]);
        let (mut server, addr) = start_server(
            connector,
            config(AccessPolicy::Allowlist(vec!["allowed.test".to_owned()])),
        )
        .await;

        let allowed = ProxyTarget::Hostname("allowed.test".to_owned());
        let echoed = echo_round_trip(addr, &allowed, 443, &Socks5Config::default())
            .await
            .expect("allowed host echoes");
        assert_eq!(echoed, b"ping");

        let denied = ProxyTarget::Hostname("denied.test".to_owned());
        let error = echo_round_trip(addr, &denied, 443, &Socks5Config::default())
            .await
            .expect_err("denied host must be refused");
        assert_eq!(
            error.kind,
            ProxyErrorKind::ConnectRejected {
                code: REP_NOT_ALLOWED
            }
        );
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn access_loopback_only_denies_non_loopback() {
        let echo = echo_server().await;
        let connector = MapConnector::new(&[("192.0.2.1", echo)]);
        let (mut server, addr) = start_server(connector, config(AccessPolicy::LoopbackOnly)).await;

        // Loopback IPv4 target is permitted.
        let target = ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let echoed = echo_round_trip(addr, &target, echo.port(), &Socks5Config::default())
            .await
            .expect("loopback permitted");
        assert_eq!(echoed, b"ping");

        // Non-loopback target is denied before any outbound connect.
        let target = ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)));
        let error = echo_round_trip(addr, &target, 443, &Socks5Config::default())
            .await
            .expect_err("non-loopback must be refused");
        assert_eq!(
            error.kind,
            ProxyErrorKind::ConnectRejected {
                code: REP_NOT_ALLOWED
            }
        );
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn required_auth_accepts_correct_credentials() {
        let echo = echo_server().await;
        let (mut server, addr) = start_server(
            MapConnector::new(&[("127.0.0.1", echo)]),
            DynamicSocksConfig {
                listen: SocketAddr::from(([127, 0, 0, 1], 0)),
                access: AccessPolicy::AllowAll,
                username: Some("alice".to_owned()),
                password: Some("hunter2".to_owned()),
                ..DynamicSocksConfig::default()
            },
        )
        .await;
        let client_config = Socks5Config {
            username: Some("alice".to_owned()),
            password: Some("hunter2".to_owned()),
            ..Socks5Config::default()
        };
        let target = ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let echoed = echo_round_trip(addr, &target, echo.port(), &client_config)
            .await
            .expect("authenticated echo");
        assert_eq!(echoed, b"ping");
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn required_auth_rejects_wrong_credentials() {
        let echo = echo_server().await;
        let (mut server, addr) = start_server(
            MapConnector::default(),
            DynamicSocksConfig {
                listen: SocketAddr::from(([127, 0, 0, 1], 0)),
                access: AccessPolicy::AllowAll,
                username: Some("alice".to_owned()),
                password: Some("hunter2".to_owned()),
                ..DynamicSocksConfig::default()
            },
        )
        .await;
        let client_config = Socks5Config {
            username: Some("alice".to_owned()),
            password: Some("wrong".to_owned()),
            ..Socks5Config::default()
        };
        let target = ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let error = echo_round_trip(addr, &target, echo.port(), &client_config)
            .await
            .expect_err("wrong credentials rejected");
        assert_eq!(error.kind, ProxyErrorKind::AuthenticationRejected);
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn required_auth_without_offer_is_refused() {
        let (mut server, addr) = start_server(
            MapConnector::default(),
            DynamicSocksConfig {
                listen: SocketAddr::from(([127, 0, 0, 1], 0)),
                access: AccessPolicy::AllowAll,
                username: Some("alice".to_owned()),
                password: Some("hunter2".to_owned()),
                ..DynamicSocksConfig::default()
            },
        )
        .await;
        // Client offers only no-auth.
        let config = Socks5Config {
            username: None,
            ..Socks5Config::default()
        };
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        let target = ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let error = socks5_connect(&mut stream, &target, 22, &config)
            .await
            .expect_err("must be refused");
        assert_eq!(error.kind, ProxyErrorKind::NoAcceptableMethod);
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn unreachable_target_gets_connection_refused() {
        // Deterministic unreachable target: the connector maps "unreachable"
        // to a closed loopback port (127.0.0.1:1), so the TCP connect fails
        // with ECONNREFUSED without depending on external DNS (some networks
        // resolve *.invalid to a blackhole address, which made the old
        // nope.invalid variant environment-dependent).
        let connector = MapConnector::new(&[("unreachable", ([127, 0, 0, 1], 1).into())]);
        let (mut server, addr) = start_server(connector, config(AccessPolicy::AllowAll)).await;
        let target = ProxyTarget::Hostname("unreachable".to_owned());
        let error = echo_round_trip(addr, &target, 443, &Socks5Config::default())
            .await
            .expect_err("unreachable target refused");
        assert_eq!(error.kind, ProxyErrorKind::ConnectRejected { code: 0x05 });
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn idle_client_times_out() {
        let (mut server, addr) = start_server(
            MapConnector::default(),
            DynamicSocksConfig {
                listen: SocketAddr::from(([127, 0, 0, 1], 0)),
                access: AccessPolicy::AllowAll,
                timeout: Duration::from_millis(50),
                ..DynamicSocksConfig::default()
            },
        )
        .await;
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        // Send nothing; the server must close the connection after the
        // handshake timeout.
        let mut buffer = [0u8; 8];
        let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
            .await
            .expect("read within 2s")
            .expect("read");
        assert_eq!(read, 0, "server must close idle connections");
        let drained = server.stop().await.expect("stop");
        assert!(drained);
    }

    #[tokio::test]
    async fn bind_failure_is_reported() {
        let occupied = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind occupied");
        let address = occupied.local_addr().expect("addr");
        let mut server = DynamicSocksServer::new(
            MapConnector::default(),
            DynamicSocksConfig {
                listen: address,
                ..DynamicSocksConfig::default()
            },
        );
        let error = server.start().await.expect_err("must fail");
        assert!(matches!(error, DynamicSocksError::BindFailed { .. }));
    }

    #[test]
    fn access_policy_matrix() {
        let allow_all = AccessPolicy::AllowAll;
        assert!(allow_all.allows(&ProxyTarget::Hostname("anything.test".to_owned())));

        let loopback = AccessPolicy::LoopbackOnly;
        assert!(loopback.allows(&ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST))));
        assert!(loopback.allows(&ProxyTarget::Hostname("localhost".to_owned())));
        assert!(!loopback.allows(&ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)))));

        let allowlist = AccessPolicy::Allowlist(vec!["db.internal".to_owned()]);
        assert!(allowlist.allows(&ProxyTarget::Hostname("db.internal".to_owned())));
        assert!(!allowlist.allows(&ProxyTarget::Hostname("web.internal".to_owned())));
        assert!(!allowlist.allows(&ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST))));
    }
}
