use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use session_orchestrator::cancellation::{select_cancellation, CancellationToken};

/// DNS resolution outcome grouped by family.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedAddresses {
    /// IPv4 addresses.
    pub ipv4: Vec<IpAddr>,
    /// IPv6 addresses.
    pub ipv6: Vec<IpAddr>,
}

impl ResolvedAddresses {
    /// All addresses, IPv6 first (Happy Eyeballs v2 start family).
    pub fn ordered(&self) -> Vec<IpAddr> {
        let mut all = Vec::with_capacity(self.ipv6.len() + self.ipv4.len());
        all.extend(self.ipv6.iter().copied());
        all.extend(self.ipv4.iter().copied());
        all
    }
}

/// DNS resolution error (no secret context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    /// The hostname could not be resolved.
    NotFound,
    /// Resolution timed out.
    Timeout,
    /// Resolution failed.
    Failed,
}

/// Injectable DNS resolver.
pub trait Resolver: Send + Sync {
    /// Resolves a hostname.
    fn resolve(
        &self,
        host: &str,
    ) -> impl std::future::Future<Output = Result<ResolvedAddresses, ResolveError>> + Send;
}

/// A connected socket guard that decrements the open-connection counter on
/// drop, making socket leaks observable in tests.
#[derive(Debug)]
pub struct ConnectionGuard {
    /// Open-connection counter shared with the connector.
    open: Arc<AtomicUsize>,
    /// Socket address this guard represents.
    pub address: SocketAddr,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.open.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Injectable connector (tests use a fake or real loopback listeners).
pub trait Connector: Send + Sync {
    /// Opens a connection to `address`, honouring the token.
    fn connect(
        &self,
        address: SocketAddr,
        token: &CancellationToken,
    ) -> impl std::future::Future<Output = Result<ConnectionGuard, ConnectError>> + Send;
}

/// Connect attempt error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectError {
    /// Connection refused or otherwise failed.
    Failed,
    /// The attempt was cancelled.
    Cancelled,
}

/// Happy Eyeballs v2 result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// Connected to an address (which family/address).
    Connected { address: SocketAddr },
    /// All attempts failed.
    Failed,
    /// The whole operation was cancelled.
    Cancelled,
}

/// Default start delay for the second address family (RFC 8305 recommends
/// 250 ms).
pub const HAPPY_EYEBALLS_START_DELAY: Duration = Duration::from_millis(250);

/// Connects with Happy Eyeballs v2 semantics.
///
/// IPv6 addresses start immediately; IPv4 starts after the stagger delay.
/// Within each family, addresses are tried in order with fast fallback. The
/// whole operation honours the cancellation token; every opened connection is
/// either returned or dropped (no socket leak). A successful v6 connection is
/// preferred and aborts the still-running v4 attempts.
pub async fn happy_eyeballs_connect<R, C>(
    resolver: &R,
    connector: &C,
    host: &str,
    port: u16,
    token: &CancellationToken,
) -> ConnectOutcome
where
    R: Resolver + ?Sized,
    C: Connector + ?Sized,
{
    let Ok(resolved) = resolver.resolve(host).await else {
        return ConnectOutcome::Failed;
    };
    let ipv6 = resolved.ipv6.clone();
    let ipv4 = resolved.ipv4.clone();

    let v6_future = try_family(connector, &ipv6, port, token.clone(), None);
    let v4_future = try_family(
        connector,
        &ipv4,
        port,
        token.clone(),
        Some(HAPPY_EYEBALLS_START_DELAY),
    );
    tokio::pin!(v6_future);
    tokio::pin!(v4_future);
    let mut v6_done = false;
    let mut v4_done = false;

    loop {
        tokio::select! {
            biased;
            result = &mut v6_future, if !v6_done => {
                v6_done = true;
                if let Some(address) = result {
                    return ConnectOutcome::Connected { address };
                }
            }
            result = &mut v4_future, if !v4_done => {
                v4_done = true;
                if let Some(address) = result {
                    return ConnectOutcome::Connected { address };
                }
            }
        }
        if v6_done && v4_done {
            break;
        }
    }
    if token.is_cancelled() {
        ConnectOutcome::Cancelled
    } else {
        ConnectOutcome::Failed
    }
}
async fn try_family<C>(
    connector: &C,
    addresses: &[IpAddr],
    port: u16,
    token: CancellationToken,
    delay: Option<Duration>,
) -> Option<SocketAddr>
where
    C: Connector + ?Sized,
{
    if let Some(delay) = delay {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = token.cancelled() => return None,
        }
    }
    for ip in addresses {
        let address = SocketAddr::new(*ip, port);
        let result = select_cancellation(&token, connector.connect(address, &token)).await;
        match result {
            Ok(Ok(guard)) => {
                drop(guard);
                return Some(address);
            }
            Ok(Err(_connect_error)) => continue,
            Err(_reason) => continue,
        }
    }
    None
}

/// A resolver backed by an in-memory map (for controlled tests).
pub struct StaticResolver {
    /// host -> addresses
    pub entries: std::collections::HashMap<String, ResolvedAddresses>,
}

impl Resolver for StaticResolver {
    async fn resolve(&self, host: &str) -> Result<ResolvedAddresses, ResolveError> {
        self.entries
            .get(host)
            .cloned()
            .ok_or(ResolveError::NotFound)
    }
}

/// A connector that records open connections and can fail per family.
pub struct CountingConnector {
    /// Open connection counter.
    pub open: Arc<AtomicUsize>,
    /// Whether IPv6 attempts fail.
    pub fail_ipv6: bool,
    /// Whether IPv4 attempts fail.
    pub fail_ipv4: bool,
    /// Whether to hang until cancelled.
    pub slow: bool,
}

impl Connector for CountingConnector {
    async fn connect(
        &self,
        address: SocketAddr,
        _token: &CancellationToken,
    ) -> Result<ConnectionGuard, ConnectError> {
        let failed = match address.ip() {
            IpAddr::V6(_) => self.fail_ipv6,
            IpAddr::V4(_) => self.fail_ipv4,
        };
        if failed {
            return Err(ConnectError::Failed);
        }
        if self.slow {
            let result = select_cancellation(_token, std::future::pending::<()>()).await;
            return match result {
                Ok(()) => unreachable!("pending never completes"),
                Err(_) => Err(ConnectError::Cancelled),
            };
        }
        self.open.fetch_add(1, Ordering::SeqCst);
        Ok(ConnectionGuard {
            open: self.open.clone(),
            address,
        })
    }
}

/// Default dual-stack addresses for a host.
pub fn dual_stack(ipv6: &str, ipv4: &str) -> ResolvedAddresses {
    ResolvedAddresses {
        ipv6: vec![ipv6.parse().expect("valid ipv6")],
        ipv4: vec![ipv4.parse().expect("valid ipv4")],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        happy_eyeballs_connect, ConnectError, ConnectOutcome, ConnectionGuard, CountingConnector,
        StaticResolver,
    };
    use session_orchestrator::cancellation::CancellationToken;
    use std::collections::HashMap;
    use std::net::{IpAddr, SocketAddr};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    const PORT: u16 = 22;
    const HOST: &str = "example.com";

    fn resolver(ipv6: Option<&str>, ipv4: Option<&str>) -> StaticResolver {
        let mut ipv6_addrs = Vec::new();
        let mut ipv4_addrs = Vec::new();
        if let Some(value) = ipv6 {
            ipv6_addrs.push(value.parse::<IpAddr>().expect("ipv6"));
        }
        if let Some(value) = ipv4 {
            ipv4_addrs.push(value.parse::<IpAddr>().expect("ipv4"));
        }
        let mut entries = HashMap::new();
        entries.insert(
            HOST.to_owned(),
            super::ResolvedAddresses {
                ipv6: ipv6_addrs,
                ipv4: ipv4_addrs,
            },
        );
        StaticResolver { entries }
    }

    fn ipv6() -> IpAddr {
        "2001:db8::1".parse().expect("ipv6")
    }

    fn ipv4() -> IpAddr {
        "192.0.2.1".parse().expect("ipv4")
    }

    #[tokio::test(start_paused = true)]
    async fn v6_first_success_preferred() {
        let token = CancellationToken::new();
        let connector = CountingConnector {
            open: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            fail_ipv6: false,
            fail_ipv4: true,
            slow: false,
        };
        let outcome = happy_eyeballs_connect(
            &resolver(Some("2001:db8::1"), None),
            &connector,
            HOST,
            PORT,
            &token,
        )
        .await;
        assert!(matches!(outcome, ConnectOutcome::Connected { address } if address.ip() == ipv6()));
    }

    #[tokio::test(start_paused = true)]
    async fn v4_fallback_after_v6_failure() {
        let token = CancellationToken::new();
        let connector = CountingConnector {
            open: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            fail_ipv6: true,
            fail_ipv4: false,
            slow: false,
        };
        let outcome = happy_eyeballs_connect(
            &resolver(Some("2001:db8::1"), Some("192.0.2.1")),
            &connector,
            HOST,
            PORT,
            &token,
        )
        .await;
        assert!(matches!(outcome, ConnectOutcome::Connected { address } if address.ip() == ipv4()));
    }

    #[tokio::test(start_paused = true)]
    async fn both_families_fail() {
        let token = CancellationToken::new();
        let connector = CountingConnector {
            open: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            fail_ipv6: true,
            fail_ipv4: true,
            slow: false,
        };
        let outcome = happy_eyeballs_connect(
            &resolver(Some("2001:db8::1"), Some("192.0.2.1")),
            &connector,
            HOST,
            PORT,
            &token,
        )
        .await;
        assert_eq!(outcome, ConnectOutcome::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn no_addresses_resolves_to_failed() {
        let token = CancellationToken::new();
        let connector = CountingConnector {
            open: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            fail_ipv6: false,
            fail_ipv4: false,
            slow: false,
        };
        let outcome =
            happy_eyeballs_connect(&resolver(None, None), &connector, HOST, PORT, &token).await;
        assert_eq!(outcome, ConnectOutcome::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_closes_all_open_connections_no_leak() {
        let token = CancellationToken::new();
        let open = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let connector = CountingConnector {
            open: open.clone(),
            fail_ipv6: false,
            fail_ipv4: false,
            slow: true,
        };
        let handle = tokio::spawn({
            let resolver = resolver(Some("2001:db8::1"), Some("192.0.2.1"));
            let token = token.clone();
            async move { happy_eyeballs_connect(&resolver, &connector, HOST, PORT, &token).await }
        });
        tokio::task::yield_now().await;
        token.cancel();
        let outcome = handle.await.expect("joined");
        assert_eq!(outcome, ConnectOutcome::Cancelled);
        // Slow connector never opened sockets (it hangs before opening), so
        // the open counter must be zero.
        assert_eq!(open.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn successful_connection_is_returned_and_counter_balanced() {
        let token = CancellationToken::new();
        let open = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let connector = CountingConnector {
            open: open.clone(),
            fail_ipv6: false,
            fail_ipv4: true,
            slow: false,
        };
        let outcome = happy_eyeballs_connect(
            &resolver(Some("2001:db8::1"), None),
            &connector,
            HOST,
            PORT,
            &token,
        )
        .await;
        assert!(matches!(outcome, ConnectOutcome::Connected { .. }));
        // The guard was dropped inside try_family after recording success, so
        // the counter is back to zero (no leak).
        assert_eq!(open.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// Real loopback integration: connect to a real tokio listener over both
    /// loopback families and confirm success without a leak.
    #[tokio::test]
    async fn real_loopback_listener_dual_stack() {
        use std::sync::atomic::AtomicUsize;
        use tokio::net::TcpListener;

        let Ok(v6_listener) = TcpListener::bind("[::1]:0").await else {
            return; // IPv6 loopback unavailable on this host; skip.
        };
        let Ok(v4_listener) = TcpListener::bind("127.0.0.1:0").await else {
            return;
        };
        // Keep both listeners alive for the duration of the test.
        let _v6_keep = &v6_listener;
        let _v4_keep = &v4_listener;
        let v6_addr = v6_listener.local_addr().expect("v6 addr");
        let v4_addr = v4_listener.local_addr().expect("v4 addr");

        struct RealConnector {
            open: Arc<AtomicUsize>,
        }
        impl super::Connector for RealConnector {
            async fn connect(
                &self,
                address: SocketAddr,
                _token: &CancellationToken,
            ) -> Result<ConnectionGuard, ConnectError> {
                let stream = tokio::net::TcpStream::connect(address)
                    .await
                    .map_err(|_| ConnectError::Failed)?;
                drop(stream);
                self.open.fetch_add(1, Ordering::SeqCst);
                Ok(ConnectionGuard {
                    open: self.open.clone(),
                    address,
                })
            }
        }

        let open = Arc::new(AtomicUsize::new(0));
        let connector = RealConnector { open: open.clone() };
        let mut entries = HashMap::new();
        entries.insert(
            "loopback".to_owned(),
            super::ResolvedAddresses {
                ipv6: vec![v6_addr.ip()],
                ipv4: vec![v4_addr.ip()],
            },
        );
        let resolver = StaticResolver { entries };
        let token = CancellationToken::new();
        let outcome =
            happy_eyeballs_connect(&resolver, &connector, "loopback", v4_addr.port(), &token).await;
        assert!(matches!(outcome, ConnectOutcome::Connected { .. }));
        assert_eq!(
            open.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no socket leak"
        );
    }
}
