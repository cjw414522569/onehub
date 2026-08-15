//! Gateway target address policy and SSRF protection (T136).
//!
//! [`AddressPolicy`] is a configurable gate applied to every target a
//! gateway client asks to reach. It classifies the resolved addresses
//! (private, link-local, loopback, cloud-metadata, reserved) and the
//! destination port, applies a host allowlist when configured, and pins the
//! validated addresses so a connect-time re-resolution that returns a
//! different set is rejected (DNS rebinding guard).

use std::net::IpAddr;

/// Ports allowed by the default policy (SSH and common SSH alt ports).
pub const DEFAULT_ALLOWED_PORTS: &[u16] = &[22, 2222];

/// A configurable target-address policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressPolicy {
    /// Whether private ranges (RFC 1918, IPv6 ULA) are allowed. Default deny.
    pub allow_private: bool,
    /// Whether link-local ranges (169.254.0.0/16, fe80::/10) are allowed.
    /// Default deny.
    pub allow_link_local: bool,
    /// Whether loopback (127.0.0.0/8, ::1) is allowed. Default deny.
    pub allow_loopback: bool,
    /// Whether cloud metadata addresses (169.254.169.254,
    /// 100.100.100.200, fd00:ec2::254) are allowed. Default deny.
    pub allow_metadata: bool,
    /// Target host allowlist. Empty means any host is allowed subject to the
    /// address and port checks. Entries are lowercased exact host names or
    /// `*.suffix` wildcards.
    pub allowed_hosts: Vec<String>,
    /// Allowed destination ports. Empty means every port is denied
    /// (explicit deny-all). Default is [`DEFAULT_ALLOWED_PORTS`].
    pub allowed_ports: Vec<u16>,
    /// DNS rebinding guard. When enabled, connect-time re-resolution must
    /// return a subset of the addresses pinned at evaluation time. Default
    /// on.
    pub dns_rebinding_guard: bool,
}

impl Default for AddressPolicy {
    fn default() -> Self {
        Self {
            allow_private: false,
            allow_link_local: false,
            allow_loopback: false,
            allow_metadata: false,
            allowed_hosts: Vec::new(),
            allowed_ports: DEFAULT_ALLOWED_PORTS.to_vec(),
            dns_rebinding_guard: true,
        }
    }
}

/// Why a target was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressPolicyError {
    /// The address is in a private range and private ranges are denied.
    PrivateAddress,
    /// The address is link-local and link-local is denied.
    LinkLocalAddress,
    /// The address is loopback and loopback is denied.
    LoopbackAddress,
    /// The address is a cloud metadata endpoint and metadata is denied.
    MetadataAddress,
    /// The address is reserved / multicast / documentation space and is
    /// never a legitimate target.
    ReservedAddress,
    /// The destination port is not in the allowed set.
    PortNotAllowed,
    /// The host is not in the configured allowlist.
    HostNotAllowed,
    /// DNS resolution returned no addresses.
    NoAddresses,
    /// The connect-time DNS answer differs from the validated set.
    DnsRebindingDetected,
}

/// A validated target: the pinned addresses that passed evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    /// The host name as requested.
    pub host: String,
    /// The destination port.
    pub port: u16,
    /// The validated, pinned addresses.
    pub ips: Vec<IpAddr>,
}

/// Whether `ip` is in an RFC 1918 private range or an IPv6 unique-local
/// (`fc00::/7`) range.
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168)
        }
        IpAddr::V6(v6) => v6.segments()[0] & 0xfe00 == 0xfc00,
    }
}

/// Whether `ip` is link-local (169.254.0.0/16, fe80::/10).
pub fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 169 && o[1] == 254
        }
        IpAddr::V6(v6) => v6.segments()[0] & 0xffc0 == 0xfe80,
    }
}

/// Whether `ip` is loopback (127.0.0.0/8, ::1).
pub fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// Whether `ip` is a cloud metadata endpoint:
/// 169.254.169.254, 100.100.100.200, fd00:ec2::254, or the IPv4-mapped
/// form of 169.254.169.254.
pub fn is_metadata(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            (o[0] == 169 && o[1] == 254 && o[2] == 169 && o[3] == 254)
                || (o[0] == 100 && o[1] == 100 && o[2] == 100 && o[3] == 200)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            (s[0] == 0xfd00
                && s[1] == 0x0ec2
                && s[2] == 0
                && s[3] == 0
                && s[4] == 0
                && s[5] == 0
                && s[6] == 0
                && s[7] == 0x0254)
                || (s[0] == 0
                    && s[1] == 0
                    && s[2] == 0
                    && s[3] == 0
                    && s[4] == 0
                    && s[5] == 0xffff
                    && s[6] == 0xa9fe
                    && s[7] == 0xa9fe)
        }
    }
}

/// Whether `ip` is reserved, multicast, documentation, or otherwise never a
/// legitimate SSH target: CGNAT 100.64.0.0/10, 0.0.0.0/8, TEST-NET ranges,
/// multicast, 240.0.0.0/4, IPv6 unspecified, NAT64 64:ff9b::/96,
/// documentation 2001:db8::/32, ORCHID 2001:10::/28, multicast ff00::/8,
/// and discard 100::/64.
pub fn is_reserved(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 0
                || (o[0] == 100 && o[1] & 0xc0 == 0x40)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 192 && o[1] == 0 && o[2] == 2)
                || (o[0] == 198 && o[1] & 0xfe == 0x12)
                || (o[0] == 198 && o[1] == 51 && o[2] == 100)
                || (o[0] == 203 && o[1] == 0 && o[2] == 113)
                || o[0] >= 224
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            (s[0] == 0
                && s[1] == 0
                && s[2] == 0
                && s[3] == 0
                && s[4] == 0
                && s[5] == 0
                && s[6] == 0
                && s[7] == 0)
                || (s[0] == 0x0064 && s[1] == 0xff9b)
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] == 0x2001 && s[1] & 0xfff0 == 0x0010)
                || (s[0] == 0x0100 && s[1] == 0)
                || s[0] & 0xff00 == 0xff00
        }
    }
}

impl AddressPolicy {
    fn port_allowed(&self, port: u16) -> bool {
        self.allowed_ports.contains(&port)
    }

    fn host_allowed(&self, host: &str) -> bool {
        if self.allowed_hosts.is_empty() {
            return true;
        }
        let host = host.to_ascii_lowercase();
        self.allowed_hosts.iter().any(|pattern| {
            let pattern = pattern.to_ascii_lowercase();
            if let Some(suffix) = pattern.strip_prefix("*.") {
                // A wildcard matches subdomains only; the apex must be
                // listed explicitly.
                host.ends_with(&format!(".{suffix}"))
            } else {
                host == pattern
            }
        })
    }

    /// Rejects a single address according to the policy.
    pub fn check_ip(&self, ip: IpAddr) -> Result<(), AddressPolicyError> {
        if is_metadata(ip) && !self.allow_metadata {
            return Err(AddressPolicyError::MetadataAddress);
        }
        if is_loopback(ip) && !self.allow_loopback {
            return Err(AddressPolicyError::LoopbackAddress);
        }
        if is_link_local(ip) && !self.allow_link_local {
            return Err(AddressPolicyError::LinkLocalAddress);
        }
        if is_private(ip) && !self.allow_private {
            return Err(AddressPolicyError::PrivateAddress);
        }
        if is_reserved(ip) {
            return Err(AddressPolicyError::ReservedAddress);
        }
        Ok(())
    }

    /// Evaluates a target: port policy, host allowlist, then every resolved
    /// address. On success returns the pinned [`ResolvedTarget`] whose IP set
    /// must be re-verified with [`AddressPolicy::verify_still_valid`] at
    /// connect time.
    pub fn evaluate(
        &self,
        host: &str,
        port: u16,
        resolve: impl Fn(&str) -> Vec<IpAddr>,
    ) -> Result<ResolvedTarget, AddressPolicyError> {
        if !self.port_allowed(port) {
            return Err(AddressPolicyError::PortNotAllowed);
        }
        if !self.host_allowed(host) {
            return Err(AddressPolicyError::HostNotAllowed);
        }
        let ips = resolve(host);
        if ips.is_empty() {
            return Err(AddressPolicyError::NoAddresses);
        }
        for ip in &ips {
            self.check_ip(*ip)?;
        }
        Ok(ResolvedTarget {
            host: host.to_owned(),
            port,
            ips,
        })
    }

    /// DNS rebinding guard: re-resolves the host and requires every returned
    /// address to be in the pinned set from evaluation. When the guard is
    /// disabled this always returns `Ok`.
    pub fn verify_still_valid(
        &self,
        target: &ResolvedTarget,
        resolve: impl Fn(&str) -> Vec<IpAddr>,
    ) -> Result<(), AddressPolicyError> {
        if !self.dns_rebinding_guard {
            return Ok(());
        }
        let now = resolve(&target.host);
        if now.is_empty() {
            return Err(AddressPolicyError::NoAddresses);
        }
        if now.iter().any(|ip| !target.ips.contains(ip)) {
            return Err(AddressPolicyError::DnsRebindingDetected);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{is_metadata, AddressPolicy, AddressPolicyError, ResolvedTarget};

    fn parse_ip(text: &str) -> std::net::IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn private_ipv4_denied_by_default() {
        let policy = AddressPolicy::default();
        for address in ["10.0.0.5", "172.16.0.1", "172.31.255.254", "192.168.1.10"] {
            assert_eq!(
                policy.evaluate("host", 22, |_| vec![parse_ip(address)]),
                Err(AddressPolicyError::PrivateAddress),
                "private {address} must be denied"
            );
        }
    }

    #[test]
    fn private_ipv6_denied_by_default() {
        let policy = AddressPolicy::default();
        for address in ["fc00::1", "fd00::1", "fdb8:1234::1"] {
            assert_eq!(
                policy.evaluate("host", 22, |_| vec![parse_ip(address)]),
                Err(AddressPolicyError::PrivateAddress),
                "ULA {address} must be denied"
            );
        }
    }

    #[test]
    fn link_local_ipv4_denied_by_default() {
        let policy = AddressPolicy::default();
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![parse_ip("169.254.10.20")]),
            Err(AddressPolicyError::LinkLocalAddress)
        );
    }

    #[test]
    fn link_local_ipv6_denied_by_default() {
        let policy = AddressPolicy::default();
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![parse_ip("fe80::1")]),
            Err(AddressPolicyError::LinkLocalAddress)
        );
    }

    #[test]
    fn loopback_denied_by_default() {
        let policy = AddressPolicy::default();
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![parse_ip("127.0.0.1")]),
            Err(AddressPolicyError::LoopbackAddress)
        );
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![parse_ip("::1")]),
            Err(AddressPolicyError::LoopbackAddress)
        );
    }

    #[test]
    fn cloud_metadata_ipv4_denied_by_default() {
        let policy = AddressPolicy::default();
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![parse_ip("169.254.169.254")]),
            Err(AddressPolicyError::MetadataAddress)
        );
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![parse_ip("100.100.100.200")]),
            Err(AddressPolicyError::MetadataAddress)
        );
        assert!(is_metadata(parse_ip("169.254.169.254")));
        assert!(is_metadata(parse_ip("100.100.100.200")));
    }

    #[test]
    fn cloud_metadata_ipv6_denied_by_default() {
        let policy = AddressPolicy::default();
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![parse_ip("fd00:ec2::254")]),
            Err(AddressPolicyError::MetadataAddress)
        );
    }

    #[test]
    fn ipv4_mapped_metadata_denied_by_default() {
        let policy = AddressPolicy::default();
        let mapped: std::net::IpAddr = "::ffff:169.254.169.254".parse().unwrap();
        assert!(is_metadata(mapped));
        assert_eq!(
            policy.evaluate("host", 22, |_| vec![mapped]),
            Err(AddressPolicyError::MetadataAddress)
        );
    }

    #[test]
    fn public_addresses_allowed_by_default() {
        let policy = AddressPolicy::default();
        let target = policy
            .evaluate("host", 22, |_| {
                vec![parse_ip("93.184.216.34"), parse_ip("2606:2800:220:1::")]
            })
            .unwrap();
        assert_eq!(target.port, 22);
        assert_eq!(target.ips.len(), 2);
        assert_eq!(
            target,
            ResolvedTarget {
                host: "host".to_owned(),
                port: 22,
                ips: vec![parse_ip("93.184.216.34"), parse_ip("2606:2800:220:1::")],
            }
        );
    }

    #[test]
    fn non_ssh_port_denied_by_default() {
        let policy = AddressPolicy::default();
        for port in [21, 23, 25, 53, 80, 443, 3306, 6379, 8080] {
            assert_eq!(
                policy.evaluate("host", port, |_| vec![parse_ip("93.184.216.34")]),
                Err(AddressPolicyError::PortNotAllowed),
                "port {port} must be denied"
            );
        }
    }

    #[test]
    fn configured_extra_port_allowed() {
        let policy = AddressPolicy {
            allowed_ports: vec![22, 2222, 8022],
            ..AddressPolicy::default()
        };
        assert_eq!(
            policy
                .evaluate("host", 8022, |_| vec![parse_ip("93.184.216.34")])
                .unwrap()
                .port,
            8022
        );
        assert_eq!(
            policy.evaluate("host", 443, |_| vec![parse_ip("93.184.216.34")]),
            Err(AddressPolicyError::PortNotAllowed)
        );
    }

    #[test]
    fn private_allowed_when_configured() {
        let policy = AddressPolicy {
            allow_private: true,
            ..AddressPolicy::default()
        };
        assert_eq!(
            policy
                .evaluate("host", 22, |_| vec![parse_ip("10.0.0.5")])
                .unwrap()
                .ips,
            vec![parse_ip("10.0.0.5")]
        );
    }

    #[test]
    fn host_allowlist_required() {
        let policy = AddressPolicy {
            allowed_hosts: vec!["db.internal".to_owned()],
            ..AddressPolicy::default()
        };
        assert!(policy
            .evaluate("db.internal", 22, |_| vec![parse_ip("93.184.216.34")])
            .is_ok());
        assert_eq!(
            policy.evaluate("other.example", 22, |_| vec![parse_ip("93.184.216.34")]),
            Err(AddressPolicyError::HostNotAllowed)
        );
        assert_eq!(
            policy.evaluate("DB.INTERNAL", 22, |_| vec![parse_ip("93.184.216.34")]),
            Ok(ResolvedTarget {
                host: "DB.INTERNAL".to_owned(),
                port: 22,
                ips: vec![parse_ip("93.184.216.34")],
            })
        );
    }

    #[test]
    fn host_allowlist_wildcard_suffix() {
        let policy = AddressPolicy {
            allowed_hosts: vec!["*.example.com".to_owned()],
            ..AddressPolicy::default()
        };
        assert!(policy
            .evaluate("relay.example.com", 22, |_| vec![parse_ip("93.184.216.34")])
            .is_ok());
        assert_eq!(
            policy.evaluate("example.com", 22, |_| vec![parse_ip("93.184.216.34")]),
            Err(AddressPolicyError::HostNotAllowed)
        );
        assert_eq!(
            policy.evaluate("relay.example.org", 22, |_| vec![parse_ip("93.184.216.34")]),
            Err(AddressPolicyError::HostNotAllowed)
        );
    }

    #[test]
    fn dns_rebinding_pinned_ip_change_rejected() {
        let policy = AddressPolicy::default();
        let target = policy
            .evaluate("safe.example", 22, |_| vec![parse_ip("93.184.216.34")])
            .unwrap();
        // Attack: the second resolution returns the private address.
        assert_eq!(
            policy.verify_still_valid(&target, |_| vec![parse_ip("10.0.0.5")]),
            Err(AddressPolicyError::DnsRebindingDetected)
        );
        assert_eq!(
            policy.verify_still_valid(&target, |_| vec![
                parse_ip("93.184.216.34"),
                parse_ip("10.0.0.5")
            ]),
            Err(AddressPolicyError::DnsRebindingDetected)
        );
    }

    #[test]
    fn dns_rebinding_same_answer_allowed() {
        let policy = AddressPolicy::default();
        let target = policy
            .evaluate("safe.example", 22, |_| {
                vec![parse_ip("93.184.216.34"), parse_ip("93.184.216.35")]
            })
            .unwrap();
        assert!(policy
            .verify_still_valid(&target, |_| vec![
                parse_ip("93.184.216.35"),
                parse_ip("93.184.216.34")
            ])
            .is_ok());
        // Guard disabled: the change is accepted.
        let lax = AddressPolicy {
            dns_rebinding_guard: false,
            ..policy.clone()
        };
        assert!(lax
            .verify_still_valid(&target, |_| vec![parse_ip("10.0.0.5")])
            .is_ok());
    }

    #[test]
    fn empty_resolution_rejected() {
        let policy = AddressPolicy::default();
        assert_eq!(
            policy.evaluate("host", 22, |_| Vec::new()),
            Err(AddressPolicyError::NoAddresses)
        );
    }

    #[test]
    fn reserved_and_multicast_denied() {
        let policy = AddressPolicy::default();
        for address in [
            "0.0.0.1",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
            "2001:db8::1",
            "ff02::1",
            "::",
        ] {
            assert_eq!(
                policy.evaluate("host", 22, |_| vec![parse_ip(address)]),
                Err(AddressPolicyError::ReservedAddress),
                "{address} must be denied as reserved"
            );
        }
    }
}
