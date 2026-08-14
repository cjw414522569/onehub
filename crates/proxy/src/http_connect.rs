//! HTTP CONNECT proxy client (T051).
//!
//! Sends an `HTTP/1.1 CONNECT host:port` request, optionally with a
//! `Proxy-Authorization` header, and validates the proxy's 2xx response. The
//! response header block is read byte-wise with a size cap so no tunnel bytes
//! are consumed.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{ProxyError, ProxyErrorKind};

/// Maximum header block we will accept (defends against unbounded reads).
pub const MAX_HEADER_BYTES: usize = 16 * 1024;

/// HTTP CONNECT client configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConnectConfig {
    /// Optional `Proxy-Authorization` header value (never logged).
    pub proxy_authorization: Option<String>,
    /// Handshake timeout.
    pub timeout: Duration,
}

impl Default for HttpConnectConfig {
    fn default() -> Self {
        Self {
            proxy_authorization: None,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Builds the CONNECT request text for `host:port`.
pub fn build_connect_request(host: &str, port: u16, config: &HttpConnectConfig) -> String {
    let authority = format!("{host}:{port}");
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(authorization) = &config.proxy_authorization {
        request.push_str(&format!("Proxy-Authorization: {authorization}\r\n"));
    }
    request.push_str("\r\n");
    request
}

/// Performs the HTTP CONNECT handshake: writes the request, reads the status
/// line and headers, and requires a 2xx status. Leaves the stream positioned
/// exactly after the header terminator so tunnel bytes are untouched.
pub async fn http_connect<S>(
    stream: &mut S,
    host: &str,
    port: u16,
    config: &HttpConnectConfig,
) -> Result<(), ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let handshake = async {
        let request = build_connect_request(host, port, config);
        stream.write_all(request.as_bytes()).await?;
        let header = read_header_block(stream).await?;
        let status = parse_status_code(&header)
            .ok_or_else(|| ProxyError::protocol("CONNECT response has no status line"))?;
        if !(200..300).contains(&status) {
            return Err(ProxyError::new(
                ProxyErrorKind::HttpStatus { code: status },
                format!("CONNECT rejected with HTTP {status}"),
            ));
        }
        Ok::<(), ProxyError>(())
    };
    match tokio::time::timeout(config.timeout, handshake).await {
        Ok(result) => result,
        Err(_) => Err(ProxyError::new(
            ProxyErrorKind::Timeout,
            format!("HTTP CONNECT timed out after {:?}", config.timeout),
        )),
    }
}

/// Reads a CRLF-CRLF terminated header block, one byte at a time, capped at
/// [`MAX_HEADER_BYTES`].
async fn read_header_block<S>(stream: &mut S) -> Result<Vec<u8>, ProxyError>
where
    S: AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    loop {
        let mut one = [0u8; 1];
        stream
            .read_exact(&mut one)
            .await
            .map_err(|_| ProxyError::new(ProxyErrorKind::Io, "read CONNECT response"))?;
        bytes.push(one[0]);
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(ProxyError::protocol("CONNECT response header too large"));
        }
        if bytes.ends_with(b"\r\n\r\n") {
            return Ok(bytes);
        }
    }
}

/// Extracts the HTTP status code from a response header block.
pub fn parse_status_code(header: &[u8]) -> Option<u16> {
    let text = String::from_utf8_lossy(header);
    let line = text.split("\r\n").next()?;
    let mut parts = line.split_whitespace();
    let _ = parts.next()?; // HTTP/1.1
    let code = parts.next()?;
    code.parse::<u16>().ok()
}
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::duplex;

    use super::{build_connect_request, http_connect, parse_status_code, HttpConnectConfig};
    use crate::{ProxyErrorKind, ProxyTarget};

    #[test]
    fn connect_request_without_auth() {
        let request = build_connect_request("example.test", 443, &HttpConnectConfig::default());
        assert_eq!(
            request,
            "CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443\r\n\r\n"
        );
        assert!(!request.contains("Proxy-Authorization"));
    }

    #[test]
    fn connect_request_with_proxy_authorization() {
        let config = HttpConnectConfig {
            proxy_authorization: Some("Basic dXNlcjpwYXNz".to_owned()),
            ..HttpConnectConfig::default()
        };
        let request = build_connect_request("example.test", 443, &config);
        assert!(request.starts_with("CONNECT example.test:443 HTTP/1.1\r\n"));
        assert!(request.contains("Host: example.test:443\r\n"));
        assert!(request.contains("Proxy-Authorization: Basic dXNlcjpwYXNz\r\n"));
        assert!(request.ends_with("\r\n\r\n"));
    }

    #[test]
    fn status_code_parsing() {
        assert_eq!(
            parse_status_code(b"HTTP/1.1 200 Connection established\r\n\r\n"),
            Some(200)
        );
        assert_eq!(
            parse_status_code(b"HTTP/1.1 407 Proxy Authentication Required\r\nX: y\r\n\r\n"),
            Some(407)
        );
        assert_eq!(parse_status_code(b"garbage"), None);
    }

    #[tokio::test]
    async fn connect_success_over_duplex() {
        let (mut client, mut server) = duplex(4096);
        let config = HttpConnectConfig::default();
        let client_future = http_connect(&mut client, "example.test", 443, &config);
        let server_future = async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buffer = Vec::new();
            let mut one = [0u8; 1];
            loop {
                server.read_exact(&mut one).await.unwrap();
                buffer.push(one[0]);
                if buffer.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            assert!(String::from_utf8_lossy(&buffer)
                .starts_with("CONNECT example.test:443 HTTP/1.1\r\n"));
            server
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .unwrap();
        };
        let (client_result, _) = tokio::join!(client_future, server_future);
        client_result.expect("connect succeeds");
    }

    #[tokio::test]
    async fn connect_non_2xx_status_is_rejected() {
        let (mut client, mut server) = duplex(4096);
        let config = HttpConnectConfig::default();
        let client_future = http_connect(&mut client, "example.test", 443, &config);
        let server_future = async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buffer = Vec::new();
            let mut one = [0u8; 1];
            loop {
                server.read_exact(&mut one).await.unwrap();
                buffer.push(one[0]);
                if buffer.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            server
                .write_all(
                    b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                )
                .await
                .unwrap();
        };
        let (client_result, _) = tokio::join!(client_future, server_future);
        let error = client_result.unwrap_err();
        assert_eq!(error.kind, ProxyErrorKind::HttpStatus { code: 407 });
        assert_eq!(error.stable_code(), "E_PROXY_HTTP_STATUS");
    }

    #[tokio::test]
    async fn connect_times_out_when_server_is_silent() {
        let (mut client, mut server) = duplex(4096);
        let config = HttpConnectConfig {
            timeout: Duration::from_millis(30),
            ..HttpConnectConfig::default()
        };
        let server_handle = tokio::spawn(async move {
            // Never respond; wait for the client to close.
            use tokio::io::AsyncReadExt;
            let mut sink = Vec::new();
            let _ = server.read_to_end(&mut sink).await;
        });
        let error = http_connect(&mut client, "example.test", 443, &config)
            .await
            .expect_err("must time out");
        assert_eq!(error.kind, ProxyErrorKind::Timeout);
        drop(client); // close so the silent server sees EOF and exits
        server_handle.await.expect("server joined");
    }

    #[test]
    fn proxy_target_host_str() {
        assert_eq!(
            ProxyTarget::Hostname("example.test".to_owned()).host_str(),
            "example.test"
        );
    }
}
