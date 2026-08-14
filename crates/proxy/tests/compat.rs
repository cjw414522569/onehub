//! T051 proxy protocol compatibility matrix over real loopback sockets.
//!
//! Matrix axes: protocol (SOCKS5 / HTTP CONNECT) x authentication (none /
//! RFC 1929 / Proxy-Authorization) x DNS policy (remote vs local resolve) x
//! address family (IPv4 / IPv6 literals) x timeout x reply codes.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proxy::{
    http_connect, parse_status_code, socks5_connect, DnsPolicy, HttpConnectConfig, ProxyErrorKind,
    ProxyTarget, Socks5Config, REP_CONNECTION_REFUSED, REP_SUCCESS,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------------
// SOCKS5 in-process test server
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Socks5ServerConfig {
    require_auth: bool,
    username: String,
    password: String,
    reply_code: u8,
    stall: bool,
    refuse_method: bool,
}

impl Default for Socks5ServerConfig {
    fn default() -> Self {
        Self {
            require_auth: false,
            username: String::new(),
            password: String::new(),
            reply_code: REP_SUCCESS,
            stall: false,
            refuse_method: false,
        }
    }
}

struct Socks5Server {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<(ProxyTarget, u16)>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl Socks5Server {
    async fn spawn(config: Socks5ServerConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_clone = requests.clone();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            if config.stall {
                let mut buffer = [0u8; 256];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
                return;
            }
            let mut head = [0u8; 2];
            stream.read_exact(&mut head).await.expect("greeting head");
            let mut methods = vec![0u8; head[1] as usize];
            stream.read_exact(&mut methods).await.expect("methods");
            if config.refuse_method {
                stream.write_all(&[0x05, 0xFF]).await.expect("refuse");
                return;
            }
            if config.require_auth {
                if !methods.contains(&0x02) {
                    stream.write_all(&[0x05, 0xFF]).await.expect("no method");
                    return;
                }
                stream
                    .write_all(&[0x05, 0x02])
                    .await
                    .expect("select userpass");
                let mut auth_head = [0u8; 2];
                stream.read_exact(&mut auth_head).await.expect("auth head");
                let mut username = vec![0u8; auth_head[1] as usize];
                stream.read_exact(&mut username).await.expect("username");
                let mut password_len = [0u8; 1];
                stream.read_exact(&mut password_len).await.expect("plen");
                let mut password = vec![0u8; password_len[0] as usize];
                stream.read_exact(&mut password).await.expect("password");
                let ok = username == config.username.as_bytes()
                    && password == config.password.as_bytes();
                let status = if ok { 0x00 } else { 0x01 };
                stream
                    .write_all(&[0x01, status])
                    .await
                    .expect("auth status");
                if !ok {
                    return;
                }
            } else {
                stream.write_all(&[0x05, 0x00]).await.expect("select none");
            }
            let request = read_socks5_request(&mut stream).await;
            let (target, port) = proxy::decode_connect_request(&request).expect("decode request");
            requests_clone.lock().expect("lock").push((target, port));
            let bind = SocketAddr::from(([127, 0, 0, 1], 12345));
            stream
                .write_all(&proxy::encode_reply(config.reply_code, bind))
                .await
                .expect("reply");
        });
        Self {
            addr,
            requests,
            handle,
        }
    }

    fn recorded(&self) -> Vec<(ProxyTarget, u16)> {
        self.requests.lock().expect("lock").clone()
    }
}

async fn read_socks5_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await.expect("request head");
    bytes.extend_from_slice(&head);
    let atyp = head[3];
    let address_length = match atyp {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut length = [0u8; 1];
            stream.read_exact(&mut length).await.expect("domain length");
            bytes.push(length[0]);
            length[0] as usize
        }
        other => panic!("unexpected atyp {other}"),
    };
    let mut rest = vec![0u8; address_length + 2];
    stream.read_exact(&mut rest).await.expect("rest");
    bytes.extend_from_slice(&rest);
    bytes
}

// ---------------------------------------------------------------------------
// HTTP CONNECT in-process test server
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct HttpRequest {
    authority: String,
    authorization: Option<String>,
}

struct HttpServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl HttpServer {
    async fn spawn(status: u16, stall: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_clone = requests.clone();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            if stall {
                let mut buffer = [0u8; 256];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
                return;
            }
            let header = read_http_header(&mut stream).await;
            let text = String::from_utf8_lossy(&header).to_string();
            let mut authority = String::new();
            let mut authorization = None;
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("CONNECT ") {
                    authority = rest
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                } else if let Some(value) = line.strip_prefix("Proxy-Authorization: ") {
                    authorization = Some(value.to_owned());
                }
            }
            requests_clone.lock().expect("lock").push(HttpRequest {
                authority,
                authorization,
            });
            let body = format!(
                "HTTP/1.1 {status} {}\r\n\r\n",
                if status == 200 {
                    "Connection established"
                } else {
                    "Rejected"
                }
            );
            stream.write_all(body.as_bytes()).await.expect("reply");
        });
        Self {
            addr,
            requests,
            handle,
        }
    }

    fn recorded(&self) -> Vec<HttpRequest> {
        self.requests.lock().expect("lock").clone()
    }
}

async fn read_http_header(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut one = [0u8; 1];
    loop {
        stream.read_exact(&mut one).await.expect("read header");
        bytes.push(one[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            return bytes;
        }
        assert!(bytes.len() < 16 * 1024, "header too large");
    }
}

// ---------------------------------------------------------------------------
// SOCKS5 matrix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn socks5_matrix_remote_resolve_domain_no_auth() {
    let server = Socks5Server::spawn(Socks5ServerConfig::default()).await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config {
        dns_policy: DnsPolicy::RemoteResolve,
        ..Socks5Config::default()
    };
    socks5_connect(
        &mut stream,
        &ProxyTarget::Hostname("example.test".to_owned()),
        443,
        &config,
    )
    .await
    .expect("connect via proxy");
    let recorded = server.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].0,
        ProxyTarget::Hostname("example.test".to_owned())
    );
    assert_eq!(recorded[0].1, 443);
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn socks5_matrix_local_resolve_ipv4() {
    let server = Socks5Server::spawn(Socks5ServerConfig::default()).await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config {
        dns_policy: DnsPolicy::LocalResolve,
        ..Socks5Config::default()
    };
    socks5_connect(
        &mut stream,
        &ProxyTarget::Ip("127.0.0.1".parse().expect("ipv4")),
        22,
        &config,
    )
    .await
    .expect("connect via proxy");
    let recorded = server.recorded();
    assert_eq!(
        recorded[0].0,
        ProxyTarget::Ip("127.0.0.1".parse().expect("ipv4"))
    );
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn socks5_matrix_local_resolve_ipv6_literal() {
    let server = Socks5Server::spawn(Socks5ServerConfig::default()).await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config {
        dns_policy: DnsPolicy::LocalResolve,
        ..Socks5Config::default()
    };
    socks5_connect(
        &mut stream,
        &ProxyTarget::Ip("::1".parse().expect("ipv6")),
        8080,
        &config,
    )
    .await
    .expect("connect via proxy");
    let recorded = server.recorded();
    assert_eq!(recorded[0].0, ProxyTarget::Ip("::1".parse().expect("ipv6")));
    assert_eq!(recorded[0].1, 8080);
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn socks5_matrix_user_pass_auth_success() {
    let server = Socks5Server::spawn(Socks5ServerConfig {
        require_auth: true,
        username: "alice".to_owned(),
        password: "hunter2".to_owned(),
        ..Socks5ServerConfig::default()
    })
    .await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config {
        username: Some("alice".to_owned()),
        password: Some("hunter2".to_owned()),
        ..Socks5Config::default()
    };
    socks5_connect(
        &mut stream,
        &ProxyTarget::Ip("127.0.0.1".parse().expect("ip")),
        22,
        &config,
    )
    .await
    .expect("authenticated connect");
    assert_eq!(server.recorded().len(), 1);
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn socks5_matrix_user_pass_auth_rejected() {
    let server = Socks5Server::spawn(Socks5ServerConfig {
        require_auth: true,
        username: "alice".to_owned(),
        password: "hunter2".to_owned(),
        ..Socks5ServerConfig::default()
    })
    .await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config {
        username: Some("alice".to_owned()),
        password: Some("wrong".to_owned()),
        ..Socks5Config::default()
    };
    let error = socks5_connect(
        &mut stream,
        &ProxyTarget::Ip("127.0.0.1".parse().expect("ip")),
        22,
        &config,
    )
    .await
    .expect_err("must be rejected");
    assert_eq!(error.kind, ProxyErrorKind::AuthenticationRejected);
    assert_eq!(error.stable_code(), "E_PROXY_AUTH_REJECTED");
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn socks5_matrix_no_acceptable_method() {
    let server = Socks5Server::spawn(Socks5ServerConfig {
        refuse_method: true,
        ..Socks5ServerConfig::default()
    })
    .await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config::default();
    let error = socks5_connect(
        &mut stream,
        &ProxyTarget::Ip("127.0.0.1".parse().expect("ip")),
        22,
        &config,
    )
    .await
    .expect_err("must fail");
    assert_eq!(error.kind, ProxyErrorKind::NoAcceptableMethod);
    assert_eq!(error.stable_code(), "E_PROXY_NO_METHOD");
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn socks5_matrix_connect_refused() {
    let server = Socks5Server::spawn(Socks5ServerConfig {
        reply_code: REP_CONNECTION_REFUSED,
        ..Socks5ServerConfig::default()
    })
    .await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config::default();
    let error = socks5_connect(
        &mut stream,
        &ProxyTarget::Ip("127.0.0.1".parse().expect("ip")),
        22,
        &config,
    )
    .await
    .expect_err("must fail");
    assert_eq!(
        error.kind,
        ProxyErrorKind::ConnectRejected {
            code: REP_CONNECTION_REFUSED
        }
    );
    assert_eq!(error.stable_code(), "E_PROXY_CONNECT_REJECTED");
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn socks5_matrix_timeout() {
    let server = Socks5Server::spawn(Socks5ServerConfig {
        stall: true,
        ..Socks5ServerConfig::default()
    })
    .await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = Socks5Config {
        timeout: Duration::from_millis(40),
        ..Socks5Config::default()
    };
    let error = socks5_connect(
        &mut stream,
        &ProxyTarget::Ip("127.0.0.1".parse().expect("ip")),
        22,
        &config,
    )
    .await
    .expect_err("must time out");
    assert_eq!(error.kind, ProxyErrorKind::Timeout);
    assert_eq!(error.stable_code(), "E_PROXY_TIMEOUT");
    drop(stream); // close so the stalled server sees EOF and exits
    server.handle.await.expect("server joined");
}

// ---------------------------------------------------------------------------
// HTTP CONNECT matrix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_connect_matrix_success_no_auth() {
    let server = HttpServer::spawn(200, false).await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    http_connect(
        &mut stream,
        "example.test",
        443,
        &HttpConnectConfig::default(),
    )
    .await
    .expect("connect via proxy");
    let recorded = server.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].authority, "example.test:443");
    assert!(recorded[0].authorization.is_none());
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn http_connect_matrix_proxy_authorization() {
    let server = HttpServer::spawn(200, false).await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = HttpConnectConfig {
        proxy_authorization: Some("Basic dXNlcjpwYXNz".to_owned()),
        ..HttpConnectConfig::default()
    };
    http_connect(&mut stream, "example.test", 8443, &config)
        .await
        .expect("connect via proxy");
    let recorded = server.recorded();
    assert_eq!(recorded[0].authority, "example.test:8443");
    assert_eq!(
        recorded[0].authorization.as_deref(),
        Some("Basic dXNlcjpwYXNz")
    );
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn http_connect_matrix_non_2xx_status() {
    let server = HttpServer::spawn(407, false).await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let error = http_connect(
        &mut stream,
        "example.test",
        443,
        &HttpConnectConfig::default(),
    )
    .await
    .expect_err("must fail");
    assert_eq!(error.kind, ProxyErrorKind::HttpStatus { code: 407 });
    assert_eq!(error.stable_code(), "E_PROXY_HTTP_STATUS");
    server.handle.await.expect("server joined");
}

#[tokio::test]
async fn http_connect_matrix_timeout() {
    let server = HttpServer::spawn(200, true).await;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let config = HttpConnectConfig {
        timeout: Duration::from_millis(40),
        ..HttpConnectConfig::default()
    };
    let error = http_connect(&mut stream, "example.test", 443, &config)
        .await
        .expect_err("must time out");
    assert_eq!(error.kind, ProxyErrorKind::Timeout);
    drop(stream); // close so the stalled server sees EOF and exits
    server.handle.await.expect("server joined");
}

#[test]
fn matrix_parse_status_code_helper() {
    assert_eq!(
        parse_status_code(b"HTTP/1.1 200 Connection established\r\n\r\n"),
        Some(200)
    );
}
