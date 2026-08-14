//! SOCKS5 proxy client (RFC 1928) with RFC 1929 username/password
//! authentication (T051).
//!
//! The protocol is implemented in-house (no external `socks5` crate) so the
//! compatibility matrix runs over real loopback sockets and the wire format
//! stays auditable.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use core_domain::proxy_chain::AddressFamily;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{ProxyError, ProxyErrorKind};

/// SOCKS5 protocol version.
pub const SOCKS5_VERSION: u8 = 0x05;
/// No-authentication method.
pub const METHOD_NO_AUTH: u8 = 0x00;
/// Username/password method (RFC 1929).
pub const METHOD_USER_PASS: u8 = 0x02;
/// No acceptable method marker.
pub const METHOD_NO_ACCEPTABLE: u8 = 0xFF;
/// Connect command.
pub const CMD_CONNECT: u8 = 0x01;
/// IPv4 address type.
pub const ATYP_IPV4: u8 = 0x01;
/// Domain name address type.
pub const ATYP_DOMAIN: u8 = 0x03;
/// IPv6 address type.
pub const ATYP_IPV6: u8 = 0x04;
/// Reply: succeeded.
pub const REP_SUCCESS: u8 = 0x00;
/// Reply: connection refused.
pub const REP_CONNECTION_REFUSED: u8 = 0x05;

/// DNS resolution policy (which ATYP the client sends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsPolicy {
    /// Send the domain name to the proxy (ATYP=DOMAINNAME); the proxy resolves.
    RemoteResolve,
    /// Resolve locally and send an IP literal (ATYP=IPv4/IPv6).
    LocalResolve,
}

/// A destination for a proxy connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyTarget {
    /// A hostname.
    Hostname(String),
    /// An IP literal.
    Ip(IpAddr),
}

impl ProxyTarget {
    /// The host portion as a string.
    pub fn host_str(&self) -> String {
        match self {
            ProxyTarget::Hostname(host) => host.clone(),
            ProxyTarget::Ip(ip) => ip.to_string(),
        }
    }
}

/// SOCKS5 client configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socks5Config {
    /// Optional RFC 1929 username. When `Some`, the client offers
    /// username/password authentication.
    pub username: Option<String>,
    /// Optional RFC 1929 password (never logged).
    pub password: Option<String>,
    /// DNS resolution policy.
    pub dns_policy: DnsPolicy,
    /// Preferred address family (used for local resolution / literal IPs).
    pub family: AddressFamily,
    /// Handshake timeout.
    pub timeout: Duration,
}

impl Default for Socks5Config {
    fn default() -> Self {
        Self {
            username: None,
            password: None,
            dns_policy: DnsPolicy::RemoteResolve,
            family: AddressFamily::Any,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Encodes a client greeting offering the given methods.
pub fn encode_greeting(methods: &[u8]) -> Vec<u8> {
    let mut bytes = vec![SOCKS5_VERSION, methods.len() as u8];
    bytes.extend_from_slice(methods);
    bytes
}

/// Decodes a server method-selection reply: `(version, method)`.
pub fn decode_method_selection(bytes: &[u8]) -> Result<(u8, u8), ProxyError> {
    if bytes.len() != 2 {
        return Err(ProxyError::protocol("method selection must be 2 bytes"));
    }
    if bytes[0] != SOCKS5_VERSION {
        return Err(ProxyError::new(
            ProxyErrorKind::Protocol,
            format!("SOCKS5 version mismatch: got 0x{:02x}, want 0x05", bytes[0]),
        ));
    }
    Ok((bytes[0], bytes[1]))
}

/// Encodes an RFC 1929 username/password auth request.
pub fn encode_auth_request(username: &str, password: &str) -> Result<Vec<u8>, ProxyError> {
    if username.len() > 255 || password.len() > 255 {
        return Err(ProxyError::protocol("username/password exceed 255 bytes"));
    }
    let mut bytes = vec![0x01, username.len() as u8];
    bytes.extend_from_slice(username.as_bytes());
    bytes.push(password.len() as u8);
    bytes.extend_from_slice(password.as_bytes());
    Ok(bytes)
}

/// Decodes an RFC 1929 auth request: `(username, password)`.
pub fn decode_auth_request(bytes: &[u8]) -> Result<(String, String), ProxyError> {
    if bytes.len() < 2 || bytes[0] != 0x01 {
        return Err(ProxyError::protocol("invalid auth version"));
    }
    let ulen = bytes[1] as usize;
    if bytes.len() < 2 + ulen + 1 {
        return Err(ProxyError::protocol("truncated auth request"));
    }
    let username = String::from_utf8(bytes[2..2 + ulen].to_vec())
        .map_err(|_| ProxyError::protocol("username is not UTF-8"))?;
    let plen = bytes[2 + ulen] as usize;
    if bytes.len() != 2 + ulen + 1 + plen {
        return Err(ProxyError::protocol("truncated auth password"));
    }
    let password = String::from_utf8(bytes[2 + ulen + 1..].to_vec())
        .map_err(|_| ProxyError::protocol("password is not UTF-8"))?;
    Ok((username, password))
}

/// Encodes a CONNECT request for `target:port` under `policy`.
pub fn encode_connect_request(
    target: &ProxyTarget,
    port: u16,
    policy: DnsPolicy,
    family: AddressFamily,
) -> Result<Vec<u8>, ProxyError> {
    let mut bytes = vec![SOCKS5_VERSION, CMD_CONNECT, 0x00];
    match (target, policy) {
        (ProxyTarget::Hostname(host), DnsPolicy::RemoteResolve) => {
            if host.is_empty() || host.len() > 255 {
                return Err(ProxyError::protocol("invalid domain name length"));
            }
            bytes.push(ATYP_DOMAIN);
            bytes.push(host.len() as u8);
            bytes.extend_from_slice(host.as_bytes());
        }
        (ProxyTarget::Hostname(host), DnsPolicy::LocalResolve) => {
            let address = pick_address(resolve_host(host, port)?, family)
                .ok_or_else(|| ProxyError::new(ProxyErrorKind::Io, format!("resolve {host}")))?;
            encode_ip(&mut bytes, address.ip());
        }
        (ProxyTarget::Ip(ip), _) => {
            encode_ip(&mut bytes, *ip);
        }
    }
    bytes.extend_from_slice(&port.to_be_bytes());
    Ok(bytes)
}

/// Decodes a CONNECT request: `(target, port)`.
pub fn decode_connect_request(bytes: &[u8]) -> Result<(ProxyTarget, u16), ProxyError> {
    if bytes.len() < 4 || bytes[0] != SOCKS5_VERSION {
        return Err(ProxyError::protocol("invalid CONNECT request"));
    }
    if bytes[1] != CMD_CONNECT {
        return Err(ProxyError::new(
            ProxyErrorKind::Unsupported,
            format!("unsupported SOCKS5 command 0x{:02x}", bytes[1]),
        ));
    }
    let atyp = bytes[3];
    let (target, rest) = match atyp {
        ATYP_IPV4 => {
            if bytes.len() < 4 + 4 + 2 {
                return Err(ProxyError::protocol("truncated IPv4 CONNECT"));
            }
            let ip = Ipv4Addr::new(bytes[4], bytes[5], bytes[6], bytes[7]);
            (ProxyTarget::Ip(IpAddr::V4(ip)), &bytes[8..])
        }
        ATYP_IPV6 => {
            if bytes.len() < 4 + 16 + 2 {
                return Err(ProxyError::protocol("truncated IPv6 CONNECT"));
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&bytes[4..20]);
            let ip = Ipv6Addr::from(octets);
            (ProxyTarget::Ip(IpAddr::V6(ip)), &bytes[20..])
        }
        ATYP_DOMAIN => {
            if bytes.len() < 5 {
                return Err(ProxyError::protocol("truncated domain CONNECT"));
            }
            let length = bytes[4] as usize;
            if bytes.len() < 5 + length + 2 {
                return Err(ProxyError::protocol("truncated domain CONNECT"));
            }
            let host = String::from_utf8(bytes[5..5 + length].to_vec())
                .map_err(|_| ProxyError::protocol("domain is not UTF-8"))?;
            (ProxyTarget::Hostname(host), &bytes[5 + length..])
        }
        other => {
            return Err(ProxyError::new(
                ProxyErrorKind::Unsupported,
                format!("unsupported ATYP 0x{other:02x}"),
            ));
        }
    };
    if rest.len() != 2 {
        return Err(ProxyError::protocol("truncated CONNECT port"));
    }
    let port = u16::from_be_bytes([rest[0], rest[1]]);
    Ok((target, port))
}

/// Encodes a server reply with a bind address.
pub fn encode_reply(rep: u8, bind: SocketAddr) -> Vec<u8> {
    let mut bytes = vec![SOCKS5_VERSION, rep, 0x00];
    encode_ip(&mut bytes, bind.ip());
    bytes.extend_from_slice(&bind.port().to_be_bytes());
    bytes
}

/// Decodes a server reply: `(reply_code, bind_address)`.
pub fn decode_reply(bytes: &[u8]) -> Result<(u8, SocketAddr), ProxyError> {
    if bytes.len() < 4 || bytes[0] != SOCKS5_VERSION {
        return Err(ProxyError::protocol("invalid SOCKS5 reply"));
    }
    if bytes[2] != 0x00 {
        return Err(ProxyError::protocol("invalid reserved byte in reply"));
    }
    let atyp = bytes[3];
    let (ip, rest) = match atyp {
        ATYP_IPV4 => {
            if bytes.len() < 4 + 4 + 2 {
                return Err(ProxyError::protocol("truncated IPv4 reply"));
            }
            (
                IpAddr::V4(Ipv4Addr::new(bytes[4], bytes[5], bytes[6], bytes[7])),
                &bytes[8..],
            )
        }
        ATYP_IPV6 => {
            if bytes.len() < 4 + 16 + 2 {
                return Err(ProxyError::protocol("truncated IPv6 reply"));
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&bytes[4..20]);
            (IpAddr::V6(Ipv6Addr::from(octets)), &bytes[20..])
        }
        ATYP_DOMAIN => {
            if bytes.len() < 5 {
                return Err(ProxyError::protocol("truncated domain reply"));
            }
            let length = bytes[4] as usize;
            if bytes.len() < 5 + length + 2 {
                return Err(ProxyError::protocol("truncated domain reply"));
            }
            let host = String::from_utf8(bytes[5..5 + length].to_vec())
                .map_err(|_| ProxyError::protocol("domain is not UTF-8"))?;
            let _ = host;
            (IpAddr::V4(Ipv4Addr::UNSPECIFIED), &bytes[5 + length..])
        }
        other => {
            return Err(ProxyError::new(
                ProxyErrorKind::Unsupported,
                format!("unsupported ATYP 0x{other:02x}"),
            ));
        }
    };
    if rest.len() != 2 {
        return Err(ProxyError::protocol("truncated reply port"));
    }
    let port = u16::from_be_bytes([rest[0], rest[1]]);
    Ok((bytes[1], SocketAddr::new(ip, port)))
}

/// Performs the full SOCKS5 handshake over `stream`: greeting, optional RFC
/// 1929 auth, CONNECT request, and reply validation, under `config.timeout`.
pub async fn socks5_connect<S>(
    stream: &mut S,
    target: &ProxyTarget,
    port: u16,
    config: &Socks5Config,
) -> Result<(), ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let handshake = async {
        let methods: Vec<u8> = if config.username.is_some() {
            vec![METHOD_NO_AUTH, METHOD_USER_PASS]
        } else {
            vec![METHOD_NO_AUTH]
        };
        stream.write_all(&encode_greeting(&methods)).await?;
        let mut selection = [0u8; 2];
        stream.read_exact(&mut selection).await?;
        let (_, method) = decode_method_selection(&selection)?;
        match method {
            METHOD_NO_ACCEPTABLE => {
                return Err(ProxyError::new(
                    ProxyErrorKind::NoAcceptableMethod,
                    "proxy has no acceptable authentication method",
                ));
            }
            METHOD_USER_PASS => {
                let username = config.username.clone().ok_or_else(|| {
                    ProxyError::protocol("proxy requested auth but none configured")
                })?;
                let password = config.password.clone().unwrap_or_default();
                let request = encode_auth_request(&username, &password)?;
                stream.write_all(&request).await?;
                let mut status = [0u8; 2];
                stream.read_exact(&mut status).await?;
                if status[0] != 0x01 || status[1] != 0x00 {
                    return Err(ProxyError::new(
                        ProxyErrorKind::AuthenticationRejected,
                        "SOCKS5 username/password rejected",
                    ));
                }
            }
            METHOD_NO_AUTH => {}
            other => {
                return Err(ProxyError::new(
                    ProxyErrorKind::Protocol,
                    format!("unexpected method selected: 0x{other:02x}"),
                ));
            }
        }
        let request = encode_connect_request(target, port, config.dns_policy, config.family)?;
        stream.write_all(&request).await?;
        let mut reply = Vec::new();
        let mut first = [0u8; 4];
        stream.read_exact(&mut first).await?;
        reply.extend_from_slice(&first);
        let atyp = first[3];
        let address_length = match atyp {
            ATYP_IPV4 => 4,
            ATYP_IPV6 => 16,
            ATYP_DOMAIN => {
                let mut length = [0u8; 1];
                stream.read_exact(&mut length).await?;
                reply.push(length[0]);
                length[0] as usize
            }
            other => {
                return Err(ProxyError::new(
                    ProxyErrorKind::Unsupported,
                    format!("unsupported reply ATYP 0x{other:02x}"),
                ));
            }
        };
        let mut rest = vec![0u8; address_length + 2];
        stream.read_exact(&mut rest).await?;
        reply.extend_from_slice(&rest);
        let (rep, _bind) = decode_reply(&reply)?;
        if rep != REP_SUCCESS {
            return Err(ProxyError::new(
                ProxyErrorKind::ConnectRejected { code: rep },
                format!("SOCKS5 connect rejected: 0x{rep:02x}"),
            ));
        }
        Ok::<(), ProxyError>(())
    };
    match tokio::time::timeout(config.timeout, handshake).await {
        Ok(result) => result,
        Err(_) => Err(ProxyError::new(
            ProxyErrorKind::Timeout,
            format!("SOCKS5 handshake timed out after {:?}", config.timeout),
        )),
    }
}

/// Appends the ATYP + address bytes for `ip`.
fn encode_ip(bytes: &mut Vec<u8>, ip: IpAddr) {
    match ip {
        IpAddr::V4(ipv4) => {
            bytes.push(ATYP_IPV4);
            bytes.extend_from_slice(&ipv4.octets());
        }
        IpAddr::V6(ipv6) => {
            bytes.push(ATYP_IPV6);
            bytes.extend_from_slice(&ipv6.octets());
        }
    }
}

/// Resolves `host` to socket addresses (blocking; used only for local DNS
/// policy on small inputs such as localhost/IP literals).
fn resolve_host(host: &str, port: u16) -> Result<Vec<SocketAddr>, ProxyError> {
    std::net::ToSocketAddrs::to_socket_addrs(&(host, port))
        .map(|iter| iter.collect())
        .map_err(|error| ProxyError::new(ProxyErrorKind::Io, format!("resolve {host}: {error}")))
}

/// Picks the first address matching `family` (Any prefers IPv6, matching
/// Happy Eyeballs v2's start family).
fn pick_address(addresses: Vec<SocketAddr>, family: AddressFamily) -> Option<SocketAddr> {
    let mut sorted = addresses;
    sorted.sort_by_key(|address| match (family, address) {
        (AddressFamily::Ipv4, SocketAddr::V4(_)) => 0,
        (AddressFamily::Ipv4, _) => 1,
        (AddressFamily::Ipv6, SocketAddr::V6(_)) => 0,
        (AddressFamily::Ipv6, _) => 1,
        (AddressFamily::Any, SocketAddr::V6(_)) => 0,
        (AddressFamily::Any, _) => 1,
    });
    sorted.into_iter().next()
}
#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::{
        decode_auth_request, decode_connect_request, decode_method_selection, decode_reply,
        encode_auth_request, encode_connect_request, encode_greeting, encode_reply, socks5_connect,
        DnsPolicy, ProxyTarget, Socks5Config, ATYP_DOMAIN, ATYP_IPV4, ATYP_IPV6, CMD_CONNECT,
        METHOD_NO_ACCEPTABLE, METHOD_NO_AUTH, METHOD_USER_PASS, REP_SUCCESS, SOCKS5_VERSION,
    };
    use crate::ProxyErrorKind;

    #[test]
    fn greeting_round_trip() {
        let greeting = encode_greeting(&[METHOD_NO_AUTH, METHOD_USER_PASS]);
        assert_eq!(
            greeting,
            vec![SOCKS5_VERSION, 2, METHOD_NO_AUTH, METHOD_USER_PASS]
        );
        let (version, method) =
            decode_method_selection(&[SOCKS5_VERSION, METHOD_USER_PASS]).unwrap();
        assert_eq!(version, SOCKS5_VERSION);
        assert_eq!(method, METHOD_USER_PASS);
        assert_eq!(
            decode_method_selection(&[0x04, METHOD_NO_AUTH]),
            Err(crate::ProxyError::protocol(
                "SOCKS5 version mismatch: got 0x04, want 0x05"
            ))
        );
        assert!(decode_method_selection(&[SOCKS5_VERSION]).is_err());
    }

    #[test]
    fn auth_request_round_trip() {
        let request = encode_auth_request("alice", "hunter2").expect("auth");
        assert_eq!(
            request,
            vec![
                0x01, 5, b'a', b'l', b'i', b'c', b'e', 7, b'h', b'u', b'n', b't', b'e', b'r', b'2'
            ]
        );
        let (username, password) = decode_auth_request(&request).expect("decode");
        assert_eq!(username, "alice");
        assert_eq!(password, "hunter2");
        assert!(decode_auth_request(&[0x02, 1, b'a', 0]).is_err());
        assert!(encode_auth_request(&"x".repeat(300), "").is_err());
    }

    #[test]
    fn connect_request_domain_remote_resolve() {
        let bytes = encode_connect_request(
            &ProxyTarget::Hostname("example.test".to_owned()),
            443,
            DnsPolicy::RemoteResolve,
            core_domain::proxy_chain::AddressFamily::Any,
        )
        .expect("encode");
        assert_eq!(bytes[0], SOCKS5_VERSION);
        assert_eq!(bytes[1], CMD_CONNECT);
        assert_eq!(bytes[3], ATYP_DOMAIN);
        let (target, port) = decode_connect_request(&bytes).expect("decode");
        assert_eq!(target, ProxyTarget::Hostname("example.test".to_owned()));
        assert_eq!(port, 443);
    }

    #[test]
    fn connect_request_ipv4_and_ipv6_literals() {
        let ipv4 = encode_connect_request(
            &ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1))),
            22,
            DnsPolicy::LocalResolve,
            core_domain::proxy_chain::AddressFamily::Any,
        )
        .expect("encode v4");
        assert_eq!(ipv4[3], ATYP_IPV4);
        let (target, port) = decode_connect_request(&ipv4).expect("decode v4");
        assert_eq!(
            target,
            ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)))
        );
        assert_eq!(port, 22);

        let ipv6 = encode_connect_request(
            &ProxyTarget::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            8080,
            DnsPolicy::LocalResolve,
            core_domain::proxy_chain::AddressFamily::Any,
        )
        .expect("encode v6");
        assert_eq!(ipv6[3], ATYP_IPV6);
        let (target, port) = decode_connect_request(&ipv6).expect("decode v6");
        assert_eq!(target, ProxyTarget::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert_eq!(port, 8080);
    }

    #[test]
    fn reply_round_trip() {
        let bind = std::net::SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), 12345));
        let reply = encode_reply(REP_SUCCESS, bind);
        let (code, decoded) = decode_reply(&reply).expect("decode reply");
        assert_eq!(code, REP_SUCCESS);
        assert_eq!(decoded, bind);
    }

    #[test]
    fn malformed_requests_are_rejected() {
        assert!(decode_connect_request(&[SOCKS5_VERSION, CMD_CONNECT, 0x00, 0x09]).is_err());
        assert!(decode_connect_request(&[SOCKS5_VERSION, 0x02, 0x00, ATYP_IPV4]).is_err());
        let (_, kind) = match decode_connect_request(&[
            SOCKS5_VERSION,
            0x02,
            0x00,
            ATYP_IPV4,
            0,
            0,
            0,
            1,
            0,
            22,
        ]) {
            Err(error) => ((), error.kind),
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(kind, ProxyErrorKind::Unsupported);
    }

    #[tokio::test]
    async fn client_handshake_succeeds_against_scripted_server() {
        use tokio::io::duplex;
        let (mut client, mut server) = duplex(4096);
        let config = Socks5Config {
            username: Some("alice".to_owned()),
            password: Some("hunter2".to_owned()),
            ..Socks5Config::default()
        };
        let target = ProxyTarget::Hostname("example.test".to_owned());
        let client_future = socks5_connect(&mut client, &target, 443, &config);
        let server_future = async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut greeting = [0u8; 4];
            server.read_exact(&mut greeting).await.unwrap();
            assert_eq!(
                greeting,
                [SOCKS5_VERSION, 2, METHOD_NO_AUTH, METHOD_USER_PASS]
            );
            server
                .write_all(&[SOCKS5_VERSION, METHOD_USER_PASS])
                .await
                .unwrap();
            let mut auth = vec![0u8; 2 + 5 + 1 + 7];
            server.read_exact(&mut auth).await.unwrap();
            let (username, password) = decode_auth_request(&auth).unwrap();
            assert_eq!(username, "alice");
            assert_eq!(password, "hunter2");
            server.write_all(&[0x01, 0x00]).await.unwrap();
            let mut head = [0u8; 4];
            server.read_exact(&mut head).await.unwrap();
            assert_eq!(head[0], SOCKS5_VERSION);
            assert_eq!(head[3], ATYP_DOMAIN);
            let mut length = [0u8; 1];
            server.read_exact(&mut length).await.unwrap();
            let mut rest = vec![0u8; length[0] as usize + 2];
            server.read_exact(&mut rest).await.unwrap();
            let bind = std::net::SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), 1234));
            server
                .write_all(&encode_reply(REP_SUCCESS, bind))
                .await
                .unwrap();
        };
        let (client_result, _) = tokio::join!(client_future, server_future);
        client_result.unwrap();
    }

    #[tokio::test]
    async fn client_handshake_rejects_when_method_not_offered() {
        use tokio::io::duplex;
        let (mut client, mut server) = duplex(4096);
        let config = Socks5Config::default();
        let target = ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let client_future = socks5_connect(&mut client, &target, 22, &config);
        let server_future = async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut greeting = [0u8; 3];
            server.read_exact(&mut greeting).await.unwrap();
            server
                .write_all(&[SOCKS5_VERSION, METHOD_USER_PASS])
                .await
                .unwrap();
        };
        let (client_result, _) = tokio::join!(client_future, server_future);
        let error = client_result.unwrap_err();
        assert_eq!(error.kind, ProxyErrorKind::Protocol);
    }

    #[tokio::test]
    async fn client_handshake_no_acceptable_method() {
        use tokio::io::duplex;
        let (mut client, mut server) = duplex(4096);
        let config = Socks5Config::default();
        let target = ProxyTarget::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let client_future = socks5_connect(&mut client, &target, 22, &config);
        let server_future = async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut greeting = [0u8; 3];
            server.read_exact(&mut greeting).await.unwrap();
            server
                .write_all(&[SOCKS5_VERSION, METHOD_NO_ACCEPTABLE])
                .await
                .unwrap();
        };
        let (client_result, _) = tokio::join!(client_future, server_future);
        let error = client_result.unwrap_err();
        assert_eq!(error.kind, ProxyErrorKind::NoAcceptableMethod);
    }
}
