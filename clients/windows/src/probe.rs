//! Network probing for connection tests (mxterm parity T003).
//!
//! Real TCP connectivity + SSH banner detection used by connection_test,
//! connection_test_profile, connection_probe_latency, and
//! connection_probe_system. No SSH handshake is performed yet (the real
//! transport is wired in a later row); this validates reachability and reads
//! the server banner to identify the remote system.

use std::io::Read;
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// A connection target.
#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub username: String,
}

impl Target {
    /// Builds a target from a request JSON (host/port/username, or
    /// connection_id resolved by the caller).
    pub fn from_request(request: &serde_json::Value) -> Self {
        let host = request
            .get("host")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let port = request
            .get("port")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(22) as u16;
        let username = request
            .get("username")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        Self {
            host,
            port,
            username,
        }
    }
}

/// Result of a TCP probe.
#[derive(Debug, Clone)]
pub struct TcpProbe {
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    /// The first line read after connect (SSH banner or empty).
    pub banner: String,
}

/// Probes TCP reachability and reads the SSH banner.
pub fn probe_tcp(target: &Target, timeout: Duration) -> TcpProbe {
    let start = Instant::now();
    let stream = match TcpStream::connect_timeout(
        &format!("{}:{}", target.host, target.port)
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().expect("addr")),
        timeout,
    ) {
        Ok(s) => s,
        Err(_) => {
            return TcpProbe {
                reachable: false,
                latency_ms: None,
                banner: String::new(),
            }
        }
    };
    let latency_ms = Some(start.elapsed().as_millis() as u64);
    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut banner = String::new();
    // Try to read a banner; SSH servers send it immediately. Non-SSH services
    // may send nothing, which is fine for reachability.
    let mut buf = [0u8; 256];
    if let Ok(n) = stream.read(&mut buf) {
        banner = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    }
    TcpProbe {
        reachable: true,
        latency_ms,
        banner,
    }
}

/// Guesses the remote OS from an SSH banner.
pub fn guess_os(banner: &str) -> (Option<String>, Option<String>) {
    let lower = banner.to_lowercase();
    if lower.contains("ubuntu") {
        (Some("ubuntu".to_string()), Some("Ubuntu".to_string()))
    } else if lower.contains("debian") {
        (Some("debian".to_string()), Some("Debian".to_string()))
    } else if lower.contains("centos") {
        (Some("centos".to_string()), Some("CentOS".to_string()))
    } else if lower.contains("rhel") || lower.contains("red hat") {
        (
            Some("rhel".to_string()),
            Some("Red Hat Enterprise Linux".to_string()),
        )
    } else if lower.contains("fedora") {
        (Some("fedora".to_string()), Some("Fedora".to_string()))
    } else if lower.contains("openssh") && lower.contains("windows") {
        (Some("windows".to_string()), Some("Windows".to_string()))
    } else if lower.contains("darwin") || lower.contains("mac") {
        (Some("macos".to_string()), Some("macOS".to_string()))
    } else if lower.contains("openssh") || lower.contains("dropbear") {
        (Some("linux".to_string()), Some("Linux".to_string()))
    } else {
        (None, None)
    }
}

/// Connection step result for the UI (ConnectionStepResult).
pub fn step_result(ok: bool, message: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "ok": ok, "message": message.into() })
}

fn step_result_verified(
    ok: bool,
    ssh_verified: bool,
    message: impl Into<String>,
) -> serde_json::Value {
    serde_json::json!({ "ok": ok, "ssh_verified": ssh_verified, "message": message.into() })
}

/// Full connection test: TCP + banner.
pub fn test_connection(request: &serde_json::Value, timeout: Duration) -> serde_json::Value {
    let target = Target::from_request(request);
    if target.host.is_empty() {
        return step_result(false, "缺少主机地址。");
    }
    let probe = probe_tcp(&target, timeout);
    if !probe.reachable {
        return step_result(
            false,
            format!("无法连接 {}:{}（TCP 不可达）。", target.host, target.port),
        );
    }
    if probe.banner.is_empty() {
        return step_result_verified(
            true,
            false,
            format!(
                "TCP 可达但未检测到 SSH 服务（{}:{}）。端口能连上但服务端无响应——可能端口不是 SSH、sshd 未运行、或防火墙/防护只放行 TCP 握手。",
                target.host, target.port
            ),
        );
    }
    step_result_verified(
        true,
        true,
        format!(
            "连接成功（{}:{}），SSH banner: {}",
            target.host, target.port, probe.banner
        ),
    )
}

/// Latency probe (connection_probe_latency).
pub fn probe_latency(request: &serde_json::Value, timeout: Duration) -> serde_json::Value {
    let target = Target::from_request(request);
    let probe = probe_tcp(&target, timeout);
    serde_json::json!({ "latency_ms": probe.latency_ms, "reachable": probe.reachable })
}

/// System probe (connection_probe_system): fills remote OS fields.
pub fn probe_system(request: &serde_json::Value, timeout: Duration) -> serde_json::Value {
    let target = Target::from_request(request);
    let probe = probe_tcp(&target, timeout);
    let (os_id, os_name) = guess_os(&probe.banner);
    serde_json::json!({
        "host": target.host,
        "port": target.port,
        "username": target.username,
        "remote_os_id": os_id,
        "remote_os_name": os_name,
        "remote_os_version": null,
        "reachable": probe.reachable,
        "banner": probe.banner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    fn spawn_echo_server() -> (String, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else {
                    continue;
                };
                let _ = s.write_all(b"SSH-2.0-OpenSSH_9.6 Ubuntu-3ubuntu3\r\n");
                let _ = s.flush();
            }
        });
        (addr.ip().to_string(), addr.port())
    }

    #[test]
    fn reachable_target_succeeds() {
        let (host, port) = spawn_echo_server();
        let req = serde_json::json!({ "host": host, "port": port, "username": "root" });
        let result = test_connection(&req, Duration::from_secs(3));
        assert_eq!(result["ok"], true);
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("SSH-2.0-OpenSSH"));
    }

    #[test]
    fn unreachable_target_fails() {
        // Port 1 on 127.0.0.1 is practically never open.
        let req = serde_json::json!({ "host": "127.0.0.1", "port": 1, "username": "root" });
        let result = test_connection(&req, Duration::from_millis(800));
        assert_eq!(result["ok"], false);
    }

    #[test]
    fn latency_reports_rtt() {
        let (host, port) = spawn_echo_server();
        let req = serde_json::json!({ "host": host, "port": port });
        let result = probe_latency(&req, Duration::from_secs(3));
        assert_eq!(result["reachable"], true);
        assert!(result["latency_ms"].as_u64().is_some());
    }

    #[test]
    fn system_probe_identifies_os() {
        let (host, port) = spawn_echo_server();
        let req = serde_json::json!({ "host": host, "port": port, "username": "root" });
        let result = probe_system(&req, Duration::from_secs(3));
        assert_eq!(result["remote_os_id"], "ubuntu");
        assert_eq!(result["remote_os_name"], "Ubuntu");
    }
}
