use serde::{Deserialize, Serialize};

use crate::credential::CredentialRef;
use crate::host::HostId;

/// Address family policy for a proxy hop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AddressFamily {
    /// Only IPv4.
    Ipv4,
    /// Only IPv6.
    Ipv6,
    /// Either family.
    Any,
}

/// How a single proxy hop reaches the next endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProxyKind {
    /// Connect directly; used as a terminating hop.
    Direct,
    /// SSH jump host (ProxyJump).
    JumpHost { host_id: HostId },
    /// SOCKS5 proxy.
    Socks5 { host_id: HostId },
    /// HTTP CONNECT proxy.
    HttpConnect { host_id: HostId },
}

/// Per-hop policy (e.g. authentication and address family).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopPolicy {
    /// Optional credential used for this hop.
    pub credential_ref: Option<CredentialRef>,
    /// Optional connect timeout in seconds.
    pub timeout_seconds: Option<u64>,
    /// Address family preference.
    pub address_family: AddressFamily,
}

impl Default for HopPolicy {
    fn default() -> Self {
        Self {
            credential_ref: None,
            timeout_seconds: None,
            address_family: AddressFamily::Any,
        }
    }
}

/// A single hop in a [`ProxyChain`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyHop {
    /// How to reach the next endpoint.
    pub kind: ProxyKind,
    /// Per-hop policy.
    pub policy: HopPolicy,
}

impl ProxyHop {
    /// Creates a hop with default policy.
    pub fn new(kind: ProxyKind) -> Self {
        Self {
            kind,
            policy: HopPolicy::default(),
        }
    }

    /// Creates a hop with an explicit policy.
    pub fn with_policy(kind: ProxyKind, policy: HopPolicy) -> Self {
        Self { kind, policy }
    }

    /// Returns the host id referenced by this hop, if any.
    pub fn host_id(&self) -> Option<&HostId> {
        match &self.kind {
            ProxyKind::Direct => None,
            ProxyKind::JumpHost { host_id }
            | ProxyKind::Socks5 { host_id }
            | ProxyKind::HttpConnect { host_id } => Some(host_id),
        }
    }
}

/// Validation outcome for a proxy chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainValidation {
    /// The chain is valid (an empty chain is a direct connection).
    Valid,
    /// The chain contains a cycle (a host is revisited).
    Cycle { host: HostId },
}

/// An ordered proxy chain ending at the target host.
///
/// An empty chain means a direct connection. Jump hosts, SOCKS5, and HTTP
/// CONNECT hops are ordered; each hop carries a per-hop policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyChain {
    hops: Vec<ProxyHop>,
}

impl ProxyChain {
    /// A direct connection (no hops).
    pub fn direct() -> Self {
        Self { hops: Vec::new() }
    }

    /// Builds a chain from hops.
    pub fn from_hops(hops: Vec<ProxyHop>) -> Self {
        Self { hops }
    }

    /// Returns the hops.
    pub fn hops(&self) -> &[ProxyHop] {
        &self.hops
    }

    /// Returns whether this is a direct connection.
    pub fn is_direct(&self) -> bool {
        self.hops.is_empty()
    }

    /// Validates the chain: no host is revisited (cycle detection over the
    /// ordered jump/proxy hosts). An empty chain is a valid direct
    /// connection.
    pub fn validate(&self) -> ChainValidation {
        let mut seen = std::collections::HashSet::new();
        for hop in &self.hops {
            if let Some(host) = hop.host_id() {
                if !seen.insert(host.clone()) {
                    return ChainValidation::Cycle { host: host.clone() };
                }
            }
        }
        ChainValidation::Valid
    }
}

/// Convenience: a single ProxyJump hop to a host.
pub fn proxy_jump(host_id: HostId) -> ProxyChain {
    ProxyChain {
        hops: vec![ProxyHop::new(ProxyKind::JumpHost { host_id })],
    }
}

/// Convenience: a multi-hop chain through jump hosts.
pub fn proxy_jump_multi(host_ids: impl IntoIterator<Item = HostId>) -> ProxyChain {
    ProxyChain {
        hops: host_ids
            .into_iter()
            .map(|host_id| ProxyHop::new(ProxyKind::JumpHost { host_id }))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{AddressFamily, ChainValidation, HopPolicy, ProxyChain, ProxyHop, ProxyKind};
    use crate::host::HostId;

    fn host(value: &str) -> HostId {
        HostId::new(value).expect("valid host id")
    }

    fn jump(value: &str) -> ProxyHop {
        ProxyHop::new(ProxyKind::JumpHost {
            host_id: host(value),
        })
    }

    #[test]
    fn direct_chain_is_valid() {
        let chain = ProxyChain::direct();
        assert!(chain.is_direct());
        assert!(chain.hops().is_empty());
        assert_eq!(chain.validate(), ChainValidation::Valid);
    }

    #[test]
    fn single_jump_is_valid() {
        let chain = ProxyChain::from_hops(vec![jump("bastion")]);
        assert!(!chain.is_direct());
        assert_eq!(chain.validate(), ChainValidation::Valid);
        assert_eq!(chain.hops().len(), 1);
    }

    #[test]
    fn multi_hop_is_valid_without_repeats() {
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b"), jump("c")]);
        assert_eq!(chain.validate(), ChainValidation::Valid);
    }

    #[test]
    fn cycle_is_detected_when_host_revisited() {
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b"), jump("a")]);
        assert_eq!(chain.validate(), ChainValidation::Cycle { host: host("a") });
    }

    #[test]
    fn socks_http_and_direct_hops_are_supported() {
        let chain = ProxyChain::from_hops(vec![
            ProxyHop::new(ProxyKind::Socks5 {
                host_id: host("socks"),
            }),
            ProxyHop::new(ProxyKind::HttpConnect {
                host_id: host("proxy"),
            }),
        ]);
        assert_eq!(chain.validate(), ChainValidation::Valid);
        let hop = &chain.hops()[0];
        assert_eq!(hop.host_id(), Some(&host("socks")));
        assert!(matches!(hop.kind, ProxyKind::Socks5 { .. }));
    }

    #[test]
    fn per_hop_policy_is_attached_and_preserved() {
        let policy = HopPolicy {
            address_family: AddressFamily::Ipv6,
            timeout_seconds: Some(30),
            ..HopPolicy::default()
        };
        let hop = ProxyHop::with_policy(ProxyKind::JumpHost { host_id: host("j") }, policy.clone());
        assert_eq!(hop.policy, policy);
        let json = serde_json::to_string(&hop).expect("serialize hop");
        let decoded: ProxyHop = serde_json::from_str(&json).expect("deserialize hop");
        assert_eq!(decoded, hop);
    }

    #[test]
    fn convenience_builders_produce_expected_chains() {
        let single = super::proxy_jump(host("bastion"));
        assert_eq!(single.validate(), ChainValidation::Valid);

        let multi = super::proxy_jump_multi([host("a"), host("b")]);
        assert_eq!(multi.validate(), ChainValidation::Valid);
        assert_eq!(multi.hops().len(), 2);
    }

    #[test]
    fn empty_from_hops_is_a_direct_chain() {
        let chain = ProxyChain::from_hops(Vec::new());
        assert!(chain.is_direct());
        assert_eq!(chain.validate(), ChainValidation::Valid);
    }

    #[test]
    fn serde_round_trip_for_chain() {
        let chain = ProxyChain::from_hops(vec![jump("a"), jump("b")]);
        let json = serde_json::to_string(&chain).expect("serialize chain");
        let decoded: ProxyChain = serde_json::from_str(&json).expect("deserialize chain");
        assert_eq!(decoded, chain);
        assert_eq!(decoded.validate(), ChainValidation::Valid);
    }

    #[test]
    fn cycle_detection_holds_for_all_small_chains() {
        // Property: for every chain of length 1..=3 over hosts {a,b}, the
        // validation reports a cycle iff some host appears more than once.
        let hosts = ["a", "b"];
        let mut chains = Vec::new();
        for length in 1..=3usize {
            let total = 1usize << (2 * length);
            for mask in 0..total {
                let mut hops = Vec::new();
                for index in 0..length {
                    let bit = (mask >> (2 * index)) & 0b11;
                    hops.push(jump(hosts[bit % 2]));
                }
                chains.push(ProxyChain::from_hops(hops));
            }
        }
        for chain in &chains {
            let mut seen = std::collections::HashSet::new();
            let mut repeated = None;
            for hop in chain.hops() {
                if let Some(host_id) = hop.host_id() {
                    if !seen.insert(host_id.clone()) {
                        repeated = Some(host_id.clone());
                        break;
                    }
                }
            }
            match chain.validate() {
                ChainValidation::Valid => assert!(repeated.is_none(), "cycle missed"),
                ChainValidation::Cycle { host } => {
                    assert_eq!(Some(host), repeated, "cycle misreported")
                }
            }
        }
    }
}
