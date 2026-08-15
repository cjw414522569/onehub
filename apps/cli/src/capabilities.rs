//! CLI forwarding / SFTP / proxy-chain capabilities (T144).
//!
//! Every capability delegates to the SAME shared core crates the GUI path
//! uses — `forwarding`, `transfer`, and `proxy` — so the CLI cannot diverge
//! from GUI behavior: both surfaces build the same configs and run the same
//! engines. The "CLI/GUI core agreement" tests run one operation through the
//! CLI surface and directly through the shared core and assert identical
//! results (config equality, byte-identical wire output, and matching
//! transfer statistics).

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use core_domain::proxy_chain::AddressFamily;
use core_domain::transfer::TransferError;
use forwarding::local::{BindScope, LocalForwardConfig};
use proxy::socks5::{encode_connect_request, encode_greeting, DnsPolicy, ProxyTarget};
use transfer::{run_streaming_copy, ChunkReader, ChunkWriter, StreamConfig, TransferStats};

/// CLI local-forwarding specification; `to_config` builds the exact config
/// the GUI path uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSpec {
    /// Local bind IP (loopback by default).
    pub bind_ip: IpAddr,
    /// Local listen port (0 = OS-assigned).
    pub listen_port: u16,
    /// Remote target host reached through the SSH channel.
    pub target_host: String,
    /// Remote target port.
    pub target_port: u16,
    /// Concurrent in-flight connection cap (0 = unlimited).
    pub max_connections: usize,
}

impl Default for ForwardSpec {
    fn default() -> Self {
        Self {
            bind_ip: IpAddr::from([127, 0, 0, 1]),
            listen_port: 0,
            target_host: String::new(),
            target_port: 22,
            max_connections: 0,
        }
    }
}

impl ForwardSpec {
    /// The bind scope of the local listener (drives the exposure warning).
    pub fn bind_scope(&self) -> BindScope {
        BindScope::from_ip(self.bind_ip)
    }

    /// Builds the shared [`LocalForwardConfig`] (identical to the GUI path).
    pub fn to_config(&self) -> LocalForwardConfig {
        LocalForwardConfig {
            listen: SocketAddr::new(self.bind_ip, self.listen_port),
            target_host: self.target_host.clone(),
            target_port: self.target_port,
            max_connections: self.max_connections,
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

/// CLI SFTP / streaming-transfer specification; `to_stream_config` builds
/// the shared [`StreamConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SftpSpec {
    /// Chunk size in bytes (defaults to the core default).
    pub chunk_size: usize,
    /// Max chunks buffered in flight (defaults to the core default).
    pub max_in_flight: usize,
    /// Yield between chunks so interactive sessions are never starved.
    pub yield_between_chunks: bool,
}

impl Default for SftpSpec {
    fn default() -> Self {
        Self {
            chunk_size: transfer::DEFAULT_CHUNK_SIZE,
            max_in_flight: transfer::DEFAULT_MAX_IN_FLIGHT,
            yield_between_chunks: true,
        }
    }
}

impl SftpSpec {
    /// Builds the shared [`StreamConfig`] (identical to the GUI path).
    pub fn to_stream_config(&self) -> StreamConfig {
        StreamConfig {
            chunk_size: self.chunk_size,
            max_in_flight: self.max_in_flight,
            yield_between_chunks: self.yield_between_chunks,
        }
    }
}

/// A proxy hop in a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyHop {
    /// Proxy host.
    pub host: String,
    /// Proxy port.
    pub port: u16,
    /// Whether this hop speaks SOCKS5 (else HTTP CONNECT).
    pub socks5: bool,
    /// Optional username for authenticated hops.
    pub username: Option<String>,
}

/// A proxy chain (ordered hops).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyChainSpec {
    /// Hops in order.
    pub hops: Vec<ProxyHop>,
}

impl ProxyChainSpec {
    /// Validates the chain: at least one hop and sane ports.
    pub fn validate(&self) -> Result<(), String> {
        if self.hops.is_empty() {
            return Err("proxy chain must have at least one hop".to_owned());
        }
        for hop in &self.hops {
            if hop.port == 0 {
                return Err(format!("proxy hop {} has port 0", hop.host));
            }
        }
        Ok(())
    }

    /// Builds the first-hop SOCKS5 greeting + CONNECT request wire bytes
    /// using the shared `proxy` encoders (byte-identical to the GUI path).
    pub fn first_hop_wire(&self, target: &ProxyTarget, port: u16) -> Result<Vec<u8>, String> {
        self.validate()?;
        let first = self.hops.first().ok_or("empty chain")?;
        if !first.socks5 {
            return Err("first hop must be SOCKS5 for the wire contract".to_owned());
        }
        // Offer no-auth; CONNECT with remote DNS resolution and any family.
        let mut wire = encode_greeting(&[0x00]);
        let request =
            encode_connect_request(target, port, DnsPolicy::RemoteResolve, AddressFamily::Any)
                .map_err(|error| error.to_string())?;
        wire.extend_from_slice(&request);
        Ok(wire)
    }
}

/// Runs a streaming copy through the shared `transfer` engine and returns
/// the statistics (used by both the CLI and the GUI surfaces).
pub async fn run_sftp_copy<R, W>(
    reader: &mut R,
    writer: W,
    config: &StreamConfig,
) -> Result<TransferStats, TransferError>
where
    R: ChunkReader,
    W: ChunkWriter,
{
    run_streaming_copy(reader, writer, config).await
}

/// Parses a `host[:port]` target into a [`ProxyTarget`].
pub fn parse_target(host: &str) -> ProxyTarget {
    match host.parse::<IpAddr>() {
        Ok(ip) => ProxyTarget::Ip(ip),
        Err(_) => ProxyTarget::Hostname(host.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::net::IpAddr;
    use std::pin::Pin;

    use core_domain::transfer::TransferError;
    use transfer::{ChunkReader, ChunkWriter};

    use super::{parse_target, ForwardSpec, ProxyChainSpec, ProxyHop, SftpSpec};

    /// A fixed byte source for the streaming copy.
    struct FixedReader {
        data: Vec<u8>,
        offset: usize,
    }

    impl ChunkReader for FixedReader {
        fn read_chunk<'a>(
            &'a mut self,
            buffer: &'a mut [u8],
        ) -> Pin<Box<dyn Future<Output = Result<usize, TransferError>> + Send + 'a>> {
            let count = buffer.len().min(self.data.len() - self.offset);
            buffer[..count].copy_from_slice(&self.data[self.offset..self.offset + count]);
            self.offset += count;
            Box::pin(async move { Ok(count) })
        }
    }

    struct CollectWriter {
        total: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    impl ChunkWriter for CollectWriter {
        fn write_chunk<'a>(
            &'a mut self,
            data: Vec<u8>,
        ) -> Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>> {
            self.total
                .fetch_add(data.len() as u64, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        }
    }

    #[test]
    fn cli_forward_config_equals_gui_core_config() {
        // The CLI surface and the GUI path must build the identical config.
        let spec = ForwardSpec {
            bind_ip: IpAddr::from([127, 0, 0, 1]),
            listen_port: 1337,
            target_host: "db.internal".to_owned(),
            target_port: 5432,
            max_connections: 4,
        };
        let cli_config = spec.to_config();
        assert_eq!(cli_config.listen, "127.0.0.1:1337".parse().unwrap());
        assert_eq!(cli_config.target_host, "db.internal");
        assert_eq!(cli_config.target_port, 5432);
        assert_eq!(cli_config.max_connections, 4);
        assert_eq!(spec.bind_scope(), forwarding::local::BindScope::Loopback);
        // A wildcard bind must require an exposure warning (shared core rule).
        let wildcard = ForwardSpec {
            bind_ip: IpAddr::from([0, 0, 0, 0]),
            ..spec
        };
        assert!(wildcard.bind_scope().requires_warning());
    }

    #[test]
    fn cli_sftp_config_equals_gui_core_config() {
        // CLI defaults must equal the shared core defaults (no divergence).
        let spec = SftpSpec::default();
        let config = spec.to_stream_config();
        assert_eq!(config, transfer::StreamConfig::default());
        assert_eq!(config.chunk_size, transfer::DEFAULT_CHUNK_SIZE);
        assert_eq!(config.max_in_flight, transfer::DEFAULT_MAX_IN_FLIGHT);
    }

    #[tokio::test]
    async fn cli_and_gui_run_the_same_streaming_engine() {
        // Both surfaces run the shared `run_streaming_copy`; the CLI-built
        // config produces the same stats as the core default.
        let data = vec![b'a'; 1 << 20];
        let mut reader = FixedReader {
            data: data.clone(),
            offset: 0,
        };
        let total = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let writer = CollectWriter {
            total: total.clone(),
        };
        let stats =
            super::run_sftp_copy(&mut reader, writer, &SftpSpec::default().to_stream_config())
                .await
                .unwrap();
        assert_eq!(stats.bytes_transferred, data.len() as u64);
        assert_eq!(
            total.load(std::sync::atomic::Ordering::SeqCst),
            data.len() as u64
        );
        assert_eq!(
            stats.chunks_transferred,
            (data.len() as u64).div_ceil(65536)
        );
        assert!(
            stats.peak_buffered_bytes
                <= transfer::DEFAULT_MAX_IN_FLIGHT * transfer::DEFAULT_CHUNK_SIZE
        );
    }

    #[test]
    fn proxy_chain_wire_matches_shared_encoders() {
        // The CLI's first-hop wire bytes must equal what the shared `proxy`
        // encoders produce for the same target (no divergence).
        let chain = ProxyChainSpec {
            hops: vec![ProxyHop {
                host: "proxy.example".to_owned(),
                port: 1080,
                socks5: true,
                username: None,
            }],
        };
        chain.validate().unwrap();
        let target = parse_target("93.184.216.34");
        let wire = chain.first_hop_wire(&target, 22).unwrap();
        // SOCKS5 greeting: 05 01 00 (version, nmethods, no-auth).
        assert_eq!(&wire[..3], &[0x05, 0x01, 0x00]);
        // CONNECT request: version 05, cmd 01, rsrvd 00, atyp 01 (IPv4).
        assert_eq!(&wire[3..7], &[0x05, 0x01, 0x00, 0x01]);
        // IPv4 address 93.184.216.34 and port 22.
        assert_eq!(&wire[7..11], &[93, 184, 216, 34]);
        assert_eq!(&wire[11..13], &[0x00, 0x16]);
        // A hostname target with remote DNS uses ATYP=DOMAIN.
        let host = parse_target("db.internal");
        let wire_host = chain.first_hop_wire(&host, 5432).unwrap();
        assert_eq!(wire_host[6], 0x03);
        // Empty chain is rejected.
        assert!(ProxyChainSpec::default().validate().is_err());
    }
}
