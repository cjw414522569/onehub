//! Network diagnostics (mxterm parity T009).
//!
//! Runs real ping / TCP / DNS / traceroute / HTTP diagnostics against a target
//! and returns the mXterm NetworkDiagnosticResult shape. TCP and DNS are done
//! in-process; ping and traceroute shell out to the platform utilities; HTTP
//! uses a raw TCP round-trip against the target port.

use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
use std::time::{Duration, Instant};

/// Runs a diagnostic (network_diagnostic_run).
pub fn run_diagnostic(kind: &str, target: &str, port: Option<u16>) -> serde_json::Value {
    let start = Instant::now();
    match kind {
        "tcp" => tcp_diagnostic(target, port, start),
        "dns" => dns_diagnostic(target, start),
        "http" => http_diagnostic(target, port, start),
        "ping" => command_diagnostic("ping", target, start),
        "trace" => command_diagnostic("tracert", target, start),
        _ => {
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": kind, "target": target, "command_label": kind,
                "ok": false, "exit_status": null, "duration_ms": duration,
                "summary": "不支持的诊断类型。", "stdout": "", "stderr": ""
            })
        }
    }
}

fn tcp_diagnostic(target: &str, port: Option<u16>, start: Instant) -> serde_json::Value {
    let port = port.unwrap_or(80);
    let addr = format!("{target}:{port}");
    match TcpStream::connect_timeout(
        &addr
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().expect("addr")),
        Duration::from_secs(5),
    ) {
        Ok(_) => {
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": "tcp", "target": target, "command_label": "tcp",
                "ok": true, "exit_status": 0, "duration_ms": duration,
                "summary": format!("TCP 连接 {addr} 成功。"), "stdout": "", "stderr": ""
            })
        }
        Err(e) => {
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": "tcp", "target": target, "command_label": "tcp",
                "ok": false, "exit_status": null, "duration_ms": duration,
                "summary": format!("TCP 连接 {addr} 失败：{e}"), "stdout": "", "stderr": format!("{e}")
            })
        }
    }
}

fn dns_diagnostic(target: &str, start: Instant) -> serde_json::Value {
    let addr = format!("{target}:53");
    match addr.to_socket_addrs() {
        Ok(mut addrs) => {
            let first = addrs.next().map(|a| a.to_string()).unwrap_or_default();
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": "dns", "target": target, "command_label": "dns",
                "ok": true, "exit_status": 0, "duration_ms": duration,
                "summary": format!("DNS 解析 {target} -> {first}"), "stdout": first, "stderr": ""
            })
        }
        Err(e) => {
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": "dns", "target": target, "command_label": "dns",
                "ok": false, "exit_status": null, "duration_ms": duration,
                "summary": format!("DNS 解析失败：{e}"), "stdout": "", "stderr": format!("{e}")
            })
        }
    }
}

fn http_diagnostic(target: &str, port: Option<u16>, start: Instant) -> serde_json::Value {
    let port = port.unwrap_or(80);
    let addr = format!("{target}:{port}");
    match TcpStream::connect_timeout(
        &addr
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().expect("addr")),
        Duration::from_secs(5),
    ) {
        Ok(mut stream) => {
            use std::io::Write;
            let request = format!("GET / HTTP/1.1\r\nHost: {target}\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(request.as_bytes());
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            let status_line = response.lines().next().unwrap_or("").to_string();
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": "http", "target": target, "command_label": "http",
                "ok": true, "exit_status": 0, "duration_ms": duration,
                "summary": format!("HTTP 请求 {addr} 完成：{status_line}"),
                "stdout": status_line, "stderr": ""
            })
        }
        Err(e) => {
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": "http", "target": target, "command_label": "http",
                "ok": false, "exit_status": null, "duration_ms": duration,
                "summary": format!("HTTP 请求失败：{e}"), "stdout": "", "stderr": format!("{e}")
            })
        }
    }
}

fn command_diagnostic(binary: &str, target: &str, start: Instant) -> serde_json::Value {
    let output = Command::new(binary).arg(target).output();
    match output {
        Ok(output) => {
            let duration = start.elapsed().as_millis() as u64;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let ok = output.status.success();
            let exit_status = output.status.code();
            let summary = if ok {
                format!("{binary} {target} 完成。")
            } else {
                format!("{binary} {target} 失败（exit={exit_status:?}）。")
            };
            serde_json::json!({
                "kind": "command", "target": target, "command_label": binary,
                "ok": ok, "exit_status": exit_status, "duration_ms": duration,
                "summary": summary, "stdout": stdout, "stderr": stderr
            })
        }
        Err(e) => {
            let duration = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "kind": "command", "target": target, "command_label": binary,
                "ok": false, "exit_status": null, "duration_ms": duration,
                "summary": format!("无法运行 {binary}：{e}"), "stdout": "", "stderr": format!("{e}")
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_diagnostic_local_echo() {
        use std::io::Write;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else {
                    continue;
                };
                let _ = s.write_all(b"ok");
            }
        });
        let result = run_diagnostic("tcp", "127.0.0.1", Some(port));
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn tcp_diagnostic_unreachable_fails() {
        let result = run_diagnostic("tcp", "127.0.0.1", Some(1));
        assert_eq!(result["ok"], false);
    }

    #[test]
    fn dns_diagnostic_localhost() {
        let result = run_diagnostic("dns", "localhost", None);
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn unknown_kind_reports_error() {
        let result = run_diagnostic("bogus", "x", None);
        assert_eq!(result["ok"], false);
    }
}
