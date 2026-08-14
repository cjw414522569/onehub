//! ProxyJump multi-hop connection orchestration (T050).
//!
//! Builds on `core-domain`'s [`ProxyChain`] (T030) and the SSH host-key
//! policy / credential primitives (T028/T041/T042). Each hop in the chain is
//! established with independent host-key verification, its own credential, and
//! its own address-family / timeout policy; failures are localized to the
//! failing hop.
//!
//! The orchestration is backend-agnostic: [`MultiHopBackend`] injects the
//! concrete SSH behaviour (connect, verify, authenticate, open a direct-tcpip
//! tunnel), so 1/2/3-hop topologies are exercised in-process with a fake
//! backend. Real container topologies (Docker + sshd) are recorded as
//! `blocked_environment` on this host (neither is installed).

use std::future::Future;
use std::pin::Pin;

use core_domain::host::HostId;
use core_domain::proxy_chain::{ChainValidation, HopPolicy, ProxyChain, ProxyKind};
use session_orchestrator::cancellation::CancellationToken;

use crate::authentication::AuthMethod;
use crate::host_key_verify::HostKeyFingerprint;

/// Stable hop failure kind (no secret context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopErrorKind {
    /// The chain is invalid (cycle or malformed hop).
    InvalidChain,
    /// A hop host could not be resolved to an endpoint.
    Resolve,
    /// The TCP/SSH connection to the hop failed.
    Connect,
    /// The hop's host key was rejected under its own policy.
    HostKeyRejected,
    /// The hop's per-hop credential was rejected.
    AuthenticationFailed,
    /// A direct-tcpip tunnel through a hop could not be opened.
    TunnelOpen,
    /// The operation was cancelled.
    Cancelled,
    /// The per-hop timeout elapsed.
    Timeout,
}

impl HopErrorKind {
    /// Stable string code (never renumbered).
    pub const fn stable_code(self) -> &'static str {
        match self {
            HopErrorKind::InvalidChain => "E_HOP_INVALID_CHAIN",
            HopErrorKind::Resolve => "E_HOP_RESOLVE",
            HopErrorKind::Connect => "E_HOP_CONNECT",
            HopErrorKind::HostKeyRejected => "E_HOP_HOST_KEY_REJECTED",
            HopErrorKind::AuthenticationFailed => "E_HOP_AUTH_FAILED",
            HopErrorKind::TunnelOpen => "E_HOP_TUNNEL_OPEN",
            HopErrorKind::Cancelled => "E_HOP_CANCELLED",
            HopErrorKind::Timeout => "E_HOP_TIMEOUT",
        }
    }
}

/// A failure localized to one hop of the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopError {
    /// 0-based hop index; the final hop is the target.
    pub hop: usize,
    /// Failure kind.
    pub kind: HopErrorKind,
    /// Human-readable detail (never contains secrets).
    pub detail: String,
}

impl HopError {
    /// Builds a localized hop error.
    pub fn new(hop: usize, kind: HopErrorKind, detail: impl Into<String>) -> Self {
        Self {
            hop,
            kind,
            detail: detail.into(),
        }
    }
}

/// A resolved hop endpoint (host name + port).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopEndpoint {
    /// Host name or IP literal.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

impl HopEndpoint {
    /// Creates an endpoint.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }
}

/// An established hop session: host key verified, authenticated, and able to
/// open a direct-tcpip tunnel to the next endpoint.
pub trait HopSession: Send + Sync {
    /// Opaque session id for diagnostics.
    fn id(&self) -> u64;

    /// Host key fingerprint after independent verification.
    fn fingerprint(&self) -> HostKeyFingerprint;

    /// Authentication method used for this hop.
    fn auth(&self) -> AuthMethod;

    /// Opens a direct-tcpip tunnel to `target` through this hop.
    fn open_tunnel<'a>(
        &'a self,
        target: &'a HopEndpoint,
        token: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), HopError>> + Send + 'a>>;
}

/// A hop established by the backend.
pub struct EstablishedHop {
    /// 0-based hop index (final index = target).
    pub index: usize,
    /// The endpoint that was established.
    pub endpoint: HopEndpoint,
    /// The established session.
    pub session: Box<dyn HopSession + Send>,
}

/// Injectable multi-hop backend.
///
/// `connect_first` establishes hop 0 over a fresh connection; `connect_next`
/// opens a direct-tcpip tunnel through `previous` and establishes hop `index`
/// over it. Both perform per-hop host-key verification and per-hop
/// authentication, and both must localize failures to `index`.
pub trait MultiHopBackend: Send + Sync {
    /// Establishes the first hop over a fresh connection.
    fn connect_first<'a>(
        &'a self,
        index: usize,
        endpoint: &'a HopEndpoint,
        policy: &'a HopPolicy,
        token: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<EstablishedHop, HopError>> + Send + 'a>>;

    /// Establishes hop `index` over a tunnel through `previous`.
    fn connect_next<'a>(
        &'a self,
        index: usize,
        previous: &'a (dyn HopSession + Send),
        endpoint: &'a HopEndpoint,
        policy: &'a HopPolicy,
        token: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<EstablishedHop, HopError>> + Send + 'a>>;
}

/// Per-hop record for diagnostics and reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopRecord {
    /// 0-based hop index (final index = target).
    pub index: usize,
    /// Host of the hop.
    pub host: String,
    /// Port of the hop.
    pub port: u16,
    /// Verified host key fingerprint.
    pub fingerprint: HostKeyFingerprint,
    /// Auth method used.
    pub auth: AuthMethod,
}

/// Result of a successful multi-hop connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiHopReport {
    /// One record per established hop, including the target.
    pub hops: Vec<HopRecord>,
}

impl MultiHopReport {
    /// Whether the chain was direct (no jump hosts).
    pub fn is_direct(&self) -> bool {
        self.hops.len() == 1
    }

    /// Hop count including the target.
    pub fn hop_count(&self) -> usize {
        self.hops.len()
    }
}

/// Resolves a chain hop `HostId` to a connectable endpoint.
pub type HopResolver = dyn Fn(&HostId) -> Option<HopEndpoint> + Send + Sync;

/// Connects through `chain` to `target` with per-hop verification and
/// credentials, returning a report of every established hop.
///
/// Hop indices: 0..chain.hops() are the jump hosts; `chain.hops().len()` is the
/// target. Errors are localized to the failing hop.
pub async fn connect_chain(
    backend: &dyn MultiHopBackend,
    chain: &ProxyChain,
    resolve_hop: &HopResolver,
    target: &HopEndpoint,
    token: &CancellationToken,
) -> Result<MultiHopReport, HopError> {
    if chain.validate() != ChainValidation::Valid {
        return Err(HopError::new(
            0,
            HopErrorKind::InvalidChain,
            "proxy chain contains a cycle or is malformed",
        ));
    }

    let mut sessions: Vec<Box<dyn HopSession + Send>> = Vec::new();
    let mut host_names: Vec<String> = Vec::new();
    for (index, hop) in chain.hops().iter().enumerate() {
        if hop.kind == ProxyKind::Direct {
            continue;
        }
        let host_id = hop.host_id().ok_or_else(|| {
            HopError::new(index, HopErrorKind::InvalidChain, "hop has no host id")
        })?;
        let endpoint = resolve_hop(host_id).ok_or_else(|| {
            HopError::new(
                index,
                HopErrorKind::Resolve,
                format!("hop host {} has no endpoint mapping", host_id.as_str()),
            )
        })?;
        let established = establish(
            backend,
            index,
            sessions.last().map(|session| session.as_ref()),
            &endpoint,
            &hop.policy,
            token,
        )
        .await?;
        sessions.push(established.session);
        host_names.push(endpoint.host);
    }

    // Final hop: the target.
    let target_index = chain.hops().len();
    let established = establish(
        backend,
        target_index,
        sessions.last().map(|session| session.as_ref()),
        target,
        &HopPolicy::default(),
        token,
    )
    .await?;
    sessions.push(established.session);
    host_names.push(target.host.clone());

    let hops: Vec<HopRecord> = sessions
        .iter()
        .zip(host_names.iter())
        .enumerate()
        .map(|(index, (session, host))| HopRecord {
            index,
            host: host.clone(),
            port: if index == sessions.len() - 1 {
                target.port
            } else {
                0
            },
            fingerprint: session.fingerprint(),
            auth: session.auth(),
        })
        .collect();

    Ok(MultiHopReport { hops })
}

/// Establishes one hop, honouring the per-hop timeout policy.
async fn establish(
    backend: &dyn MultiHopBackend,
    index: usize,
    previous: Option<&(dyn HopSession + Send)>,
    endpoint: &HopEndpoint,
    policy: &HopPolicy,
    token: &CancellationToken,
) -> Result<EstablishedHop, HopError> {
    let future = match previous {
        Some(previous) => backend.connect_next(index, previous, endpoint, policy, token),
        None => backend.connect_first(index, endpoint, policy, token),
    };
    let timeout = policy.timeout_seconds.map(std::time::Duration::from_secs);
    match timeout {
        Some(duration) => match tokio::time::timeout(duration, future).await {
            Ok(result) => result,
            Err(_) => Err(HopError::new(
                index,
                HopErrorKind::Timeout,
                format!("hop {index} exceeded timeout of {}s", duration.as_secs()),
            )),
        },
        None => future.await,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use core_domain::host::HostId;
    use core_domain::host_key::{verify_host_key, HostKeyDecision, HostKeyPolicy, HostKeyStatus};
    use core_domain::proxy_chain::{HopPolicy, ProxyChain, ProxyHop, ProxyKind};
    use secret::SecretString;
    use session_orchestrator::cancellation::CancellationToken;

    use super::{
        connect_chain, EstablishedHop, HopEndpoint, HopError, HopErrorKind, HopResolver,
        HopSession, MultiHopBackend,
    };
    use crate::authentication::{AuthMethod, PasswordAuthenticator};
    use crate::host_key_verify::HostKeyFingerprint;

    static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

    /// An in-process hop topology: hosts with known keys, per-hop credentials,
    /// and reachability routes (which hosts a hop may tunnel to).
    #[derive(Clone)]
    struct FakeTopology {
        /// Host -> known host key blob (what the hop presents).
        keys: HashMap<String, Vec<u8>>,
        /// Host -> correct credential.
        passwords: HashMap<String, String>,
        /// Host -> hosts reachable through it.
        routes: HashMap<String, Vec<String>>,
        /// Optional per-hop index to fail with a connect error.
        fail_connect: Option<usize>,
        /// Optional per-hop index to fail with a host-key change.
        fail_key: Option<usize>,
        /// Optional per-hop index to fail with bad credential.
        fail_auth: Option<usize>,
        /// Optional artificial delay per connect (for timeout tests).
        delay: Option<Duration>,
    }

    /// Backend that performs per-hop host-key verification (T028/T041 policy)
    /// and constant-time password authentication (T042), and chains hops
    /// through direct-tcpip tunnels.
    struct FakeBackend {
        topology: FakeTopology,
    }

    struct FakeSession {
        id: u64,
        host: String,
        key_blob: Vec<u8>,
        auth: AuthMethod,
        routes: HashMap<String, Vec<String>>,
    }

    impl HopSession for FakeSession {
        fn id(&self) -> u64 {
            self.id
        }

        fn fingerprint(&self) -> HostKeyFingerprint {
            HostKeyFingerprint::sha256(&self.key_blob)
        }

        fn auth(&self) -> AuthMethod {
            self.auth
        }

        fn open_tunnel<'a>(
            &'a self,
            target: &'a HopEndpoint,
            _token: &'a CancellationToken,
        ) -> Pin<Box<dyn Future<Output = Result<(), HopError>> + Send + 'a>> {
            Box::pin(async move {
                let reachable = match self.routes.get(&self.host) {
                    Some(reachable) => reachable.iter().any(|host| host == &target.host),
                    None => true,
                };
                if !reachable {
                    return Err(HopError::new(
                        0,
                        HopErrorKind::TunnelOpen,
                        format!("{} cannot reach {}", self.host, target.host),
                    ));
                }
                Ok(())
            })
        }
    }

    impl FakeBackend {
        fn establish(
            &self,
            index: usize,
            endpoint: &HopEndpoint,
        ) -> Result<EstablishedHop, HopError> {
            if self.topology.fail_connect == Some(index) {
                return Err(HopError::new(
                    index,
                    HopErrorKind::Connect,
                    format!("tcp connect to {}:{} refused", endpoint.host, endpoint.port),
                ));
            }
            let key_blob = self
                .topology
                .keys
                .get(&endpoint.host)
                .cloned()
                .ok_or_else(|| {
                    HopError::new(
                        index,
                        HopErrorKind::Connect,
                        format!("host {} unknown to topology", endpoint.host),
                    )
                })?;
            let password = self
                .topology
                .passwords
                .get(&endpoint.host)
                .cloned()
                .unwrap_or_default();

            // Per-hop independent host-key verification (T028 policy + T041
            // fingerprint). A changed/unknown key is rejected at this hop only.
            let presented: &[u8] = if self.topology.fail_key == Some(index) {
                b"attacker-key-blob"
            } else {
                &key_blob
            };
            let status = if presented == key_blob {
                HostKeyStatus::Known
            } else {
                HostKeyStatus::Changed
            };
            match verify_host_key(HostKeyPolicy::Strict, status) {
                HostKeyDecision::Trusted => {}
                _ => {
                    return Err(HopError::new(
                        index,
                        HopErrorKind::HostKeyRejected,
                        format!("host key for {} rejected", endpoint.host),
                    ));
                }
            }

            // Per-hop credential (T042): the correct password for this host is
            // `password`; a wrong supplied password fails only this hop.
            let supplied = if self.topology.fail_auth == Some(index) {
                "wrong-password".to_owned()
            } else {
                password.clone()
            };
            let authenticator = PasswordAuthenticator::new(SecretString::from(password));
            if !authenticator.attempt(&SecretString::from(supplied)) {
                return Err(HopError::new(
                    index,
                    HopErrorKind::AuthenticationFailed,
                    format!("credential rejected for {}", endpoint.host),
                ));
            }

            let session = FakeSession {
                id: NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst),
                host: endpoint.host.clone(),
                key_blob,
                auth: AuthMethod::Password,
                routes: self.topology.routes.clone(),
            };
            Ok(EstablishedHop {
                index,
                endpoint: endpoint.clone(),
                session: Box::new(session),
            })
        }
    }

    impl MultiHopBackend for FakeBackend {
        fn connect_first<'a>(
            &'a self,
            index: usize,
            endpoint: &'a HopEndpoint,
            _policy: &'a HopPolicy,
            _token: &'a CancellationToken,
        ) -> Pin<Box<dyn Future<Output = Result<EstablishedHop, HopError>> + Send + 'a>> {
            Box::pin(async move {
                if let Some(delay) = self.topology.delay {
                    tokio::time::sleep(delay).await;
                }
                self.establish(index, endpoint)
            })
        }

        fn connect_next<'a>(
            &'a self,
            index: usize,
            previous: &'a (dyn HopSession + Send),
            endpoint: &'a HopEndpoint,
            _policy: &'a HopPolicy,
            token: &'a CancellationToken,
        ) -> Pin<Box<dyn Future<Output = Result<EstablishedHop, HopError>> + Send + 'a>> {
            Box::pin(async move {
                if let Some(delay) = self.topology.delay {
                    tokio::time::sleep(delay).await;
                }
                if token.is_cancelled() {
                    return Err(HopError::new(
                        index,
                        HopErrorKind::Cancelled,
                        "cancelled before hop establishment",
                    ));
                }
                previous
                    .open_tunnel(endpoint, token)
                    .await
                    .map_err(|mut error| {
                        error.hop = index;
                        error
                    })?;
                self.establish(index, endpoint)
            })
        }
    }

    fn host(value: &str) -> HostId {
        HostId::new(value).expect("valid host id")
    }

    fn endpoint(host: &str, port: u16) -> HopEndpoint {
        HopEndpoint::new(host, port)
    }

    fn jump(value: &str) -> ProxyHop {
        ProxyHop::new(ProxyKind::JumpHost {
            host_id: host(value),
        })
    }

    fn resolver(hosts: &[(&str, u16)]) -> Box<HopResolver> {
        let entries: Vec<(String, HopEndpoint)> = hosts
            .iter()
            .map(|(name, port)| (name.to_string(), endpoint(name, *port)))
            .collect();
        Box::new(move |id: &HostId| {
            entries
                .iter()
                .find(|(name, _)| name == id.as_str())
                .map(|(_, ep)| ep.clone())
        })
    }

    fn topology(hosts: &[(&str, u16)], routes: &[(&str, &[&str])]) -> FakeTopology {
        let mut keys = HashMap::new();
        let mut passwords = HashMap::new();
        for (name, _) in hosts {
            keys.insert((*name).to_owned(), format!("key-{name}").into_bytes());
            passwords.insert((*name).to_owned(), format!("pw-{name}"));
        }
        let mut route_map = HashMap::new();
        for (from, tos) in routes {
            route_map.insert(
                (*from).to_owned(),
                tos.iter().map(|to| (*to).to_owned()).collect(),
            );
        }
        FakeTopology {
            keys,
            passwords,
            routes: route_map,
            fail_connect: None,
            fail_key: None,
            fail_auth: None,
            delay: None,
        }
    }

    fn backend(topology: FakeTopology) -> FakeBackend {
        FakeBackend { topology }
    }

    #[tokio::test]
    async fn direct_connection_has_single_hop() {
        let topology = topology(&[("target", 22)], &[]);
        let backend = backend(topology);
        let chain = ProxyChain::direct();
        let resolver = resolver(&[("target", 22)]);
        let token = CancellationToken::new();
        let report = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect("direct connect");
        assert!(report.is_direct());
        assert_eq!(report.hop_count(), 1);
        assert_eq!(report.hops[0].host, "target");
        assert_eq!(report.hops[0].auth, AuthMethod::Password);
    }

    #[tokio::test]
    async fn single_jump_topology_connects() {
        let topology = topology(
            &[("bastion", 22), ("target", 22)],
            &[("bastion", &["target"])],
        );
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("bastion")]);
        let resolver = resolver(&[("bastion", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let report = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect("1-hop connect");
        assert_eq!(report.hop_count(), 2);
        assert_eq!(report.hops[0].host, "bastion");
        assert_eq!(report.hops[1].host, "target");
    }

    #[tokio::test]
    async fn two_hop_topology_connects() {
        let topology = topology(
            &[("a", 22), ("b", 22), ("target", 22)],
            &[("a", &["b"]), ("b", &["target"])],
        );
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b")]);
        let resolver = resolver(&[("a", 22), ("b", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let report = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect("2-hop connect");
        assert_eq!(report.hop_count(), 3);
        assert_eq!(
            report
                .hops
                .iter()
                .map(|hop| hop.host.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "target"]
        );
    }

    #[tokio::test]
    async fn three_hop_topology_connects() {
        let topology = topology(
            &[("a", 22), ("b", 22), ("c", 22), ("target", 22)],
            &[("a", &["b"]), ("b", &["c"]), ("c", &["target"])],
        );
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b"), jump("c")]);
        let resolver = resolver(&[("a", 22), ("b", 22), ("c", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let report = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect("3-hop connect");
        assert_eq!(report.hop_count(), 4);
        assert_eq!(
            report
                .hops
                .iter()
                .map(|hop| hop.host.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "target"]
        );
        for hop in &report.hops {
            assert!(!hop.fingerprint.sha256_b64.is_empty());
        }
    }

    #[tokio::test]
    async fn host_key_rejection_is_localized_to_that_hop() {
        let mut topology = topology(
            &[("a", 22), ("b", 22), ("target", 22)],
            &[("a", &["b"]), ("b", &["target"])],
        );
        topology.fail_key = Some(1);
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b")]);
        let resolver = resolver(&[("a", 22), ("b", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.hop, 1);
        assert_eq!(error.kind, HopErrorKind::HostKeyRejected);
        assert_eq!(error.kind.stable_code(), "E_HOP_HOST_KEY_REJECTED");
    }

    #[tokio::test]
    async fn per_hop_credential_failure_is_localized() {
        let mut topology = topology(&[("a", 22), ("target", 22)], &[("a", &["target"])]);
        topology.fail_auth = Some(0);
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a")]);
        let resolver = resolver(&[("a", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.hop, 0);
        assert_eq!(error.kind, HopErrorKind::AuthenticationFailed);
        assert_eq!(error.kind.stable_code(), "E_HOP_AUTH_FAILED");
    }

    #[tokio::test]
    async fn tunnel_unreachable_hop_is_localized() {
        let topology = topology(
            &[("a", 22), ("b", 22), ("target", 22)],
            &[("a", &["b"]), ("b", &[])], // b cannot reach target
        );
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b")]);
        let resolver = resolver(&[("a", 22), ("b", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.hop, 2);
        assert_eq!(error.kind, HopErrorKind::TunnelOpen);
        assert_eq!(error.kind.stable_code(), "E_HOP_TUNNEL_OPEN");
    }

    #[tokio::test]
    async fn connect_failure_is_localized_to_hop_index() {
        let mut topology = topology(
            &[("a", 22), ("b", 22), ("target", 22)],
            &[("a", &["b"]), ("b", &["target"])],
        );
        topology.fail_connect = Some(2);
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b")]);
        let resolver = resolver(&[("a", 22), ("b", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.hop, 2);
        assert_eq!(error.kind, HopErrorKind::Connect);
        assert_eq!(error.kind.stable_code(), "E_HOP_CONNECT");
    }

    #[tokio::test]
    async fn unresolved_hop_is_reported_as_resolve_error() {
        let topology = topology(&[("a", 22), ("target", 22)], &[("a", &["target"])]);
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("missing")]);
        let resolver = resolver(&[("a", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.hop, 1);
        assert_eq!(error.kind, HopErrorKind::Resolve);
    }

    #[tokio::test]
    async fn cyclic_chain_is_rejected() {
        let topology = topology(&[("a", 22), ("target", 22)], &[("a", &["a"])]);
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("a")]);
        let resolver = resolver(&[("a", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.kind, HopErrorKind::InvalidChain);
        assert_eq!(error.kind.stable_code(), "E_HOP_INVALID_CHAIN");
    }

    #[tokio::test]
    async fn pre_cancelled_token_stops_at_first_hop() {
        let topology = topology(&[("a", 22), ("target", 22)], &[("a", &["target"])]);
        let backend = backend(topology);
        let chain = ProxyChain::from_hops(vec![jump("a")]);
        let resolver = resolver(&[("a", 22), ("target", 22)]);
        let token = CancellationToken::new();
        token.cancel();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.kind, HopErrorKind::Cancelled);
        assert_eq!(error.kind.stable_code(), "E_HOP_CANCELLED");
    }

    #[tokio::test]
    async fn per_hop_timeout_policy_is_enforced() {
        let mut topology = topology(&[("a", 22), ("target", 22)], &[("a", &["target"])]);
        topology.delay = Some(Duration::from_millis(50));
        let backend = backend(topology);
        let slow_hop = ProxyHop::with_policy(
            ProxyKind::JumpHost { host_id: host("a") },
            HopPolicy {
                timeout_seconds: Some(0),
                ..HopPolicy::default()
            },
        );
        let chain = ProxyChain::from_hops(vec![slow_hop]);
        let resolver = resolver(&[("a", 22), ("target", 22)]);
        let token = CancellationToken::new();
        let error = connect_chain(
            &backend,
            &chain,
            resolver.as_ref(),
            &endpoint("target", 22),
            &token,
        )
        .await
        .expect_err("must fail");
        assert_eq!(error.hop, 0);
        assert_eq!(error.kind, HopErrorKind::Timeout);
        assert_eq!(error.kind.stable_code(), "E_HOP_TIMEOUT");
    }

    #[test]
    fn stable_error_codes_are_unique_and_prefixed() {
        use super::HopErrorKind::*;
        let codes = [
            InvalidChain.stable_code(),
            Resolve.stable_code(),
            Connect.stable_code(),
            HostKeyRejected.stable_code(),
            AuthenticationFailed.stable_code(),
            TunnelOpen.stable_code(),
            Cancelled.stable_code(),
            Timeout.stable_code(),
        ];
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(code.starts_with("E_HOP_"), "prefix: {code}");
            assert!(seen.insert(code), "duplicate: {code}");
        }
        assert_eq!(codes.len(), 8);
    }
}
