//! Remote host monitoring (mxterm parity T012).
//!
//! Collects a host snapshot over a real SSH session by running standard Linux
//! commands (uptime / uname / nproc / free / df / ps / /proc/cpuinfo) and
//! parses them into the mXterm RemoteMonitorSnapshot shape. process_signal
//! sends a signal via `kill -s SIG pid`.

use std::sync::Arc;

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use serde_json::{json, Value};

use crate::sftp::SshTarget;
use crate::store::Store;

async fn exec_remote(store: &Store, connection_id: &str, command: &str) -> Result<String, String> {
    let profile = store
        .get_connection(connection_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "关联的连接不存在。".to_string())?;
    let target = SshTarget::from_request(&profile);
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(
        config,
        (target.host.as_str(), target.port),
        AcceptAllHostKey,
    )
    .await
    .map_err(|e| format!("SSH 连接失败：{e}"))?;
    let authenticated = if let Some(password) = &target.password {
        session
            .authenticate_password(target.username.as_str(), password.as_str())
            .await
            .map_err(|e| format!("SSH 认证失败：{e}"))?
            .success()
    } else if let Some(key_path) = &target.private_key_path {
        let key = load_secret_key(key_path, target.private_key_passphrase.as_deref())
            .map_err(|e| format!("私钥加载失败：{e}"))?;
        let hash_alg = session
            .best_supported_rsa_hash()
            .await
            .map_err(|e| format!("SSH 认证失败：{e}"))?
            .flatten();
        session
            .authenticate_publickey(
                target.username.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
            )
            .await
            .map_err(|e| format!("SSH 认证失败：{e}"))?
            .success()
    } else {
        false
    };
    if !authenticated {
        return Err("SSH 认证失败：凭据被拒绝。".to_string());
    }
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("SSH 通道打开失败：{e}"))?;
    channel
        .exec(true, command)
        .await
        .map_err(|e| format!("命令执行失败：{e}"))?;
    let mut stdout = String::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } => stdout.push_str(&String::from_utf8_lossy(&data)),
            russh::ChannelMsg::Eof => break,
            _ => {}
        }
    }
    Ok(stdout)
}

fn parse_uptime(text: &str) -> (Option<u64>, Option<[f64; 3]>) {
    // " 10:23:45 up 3 days,  4:12,  2 users,  load average: 0.10, 0.20, 0.30"
    let uptime = text
        .split("up ")
        .nth(1)
        .and_then(|rest| rest.split("load average:").next())
        .map(|s| s.trim().to_string());
    let seconds = uptime.map(|u| {
        let tokens: Vec<String> = u
            .split_whitespace()
            .map(|raw| {
                raw.chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == ':')
                    .collect()
            })
            .collect();
        let mut total = 0u64;
        let mut i = 0;
        while i < tokens.len() {
            if let Ok(days) = tokens[i].parse::<u64>() {
                if i + 1 < tokens.len() && (tokens[i + 1] == "day" || tokens[i + 1] == "days") {
                    total += days * 86400;
                    i += 2;
                    continue;
                }
            }
            if let Some((h, m)) = tokens[i].split_once(':') {
                total += h.parse::<u64>().unwrap_or(0) * 3600 + m.parse::<u64>().unwrap_or(0) * 60;
            }
            i += 1;
        }
        total
    });
    let load = text.split("load average:").nth(1).and_then(|rest| {
        let nums: Vec<f64> = rest
            .split(',')
            .filter_map(|s| s.trim().trim_end_matches(',').trim().parse::<f64>().ok())
            .collect();
        (nums.len() == 3).then(|| [nums[0], nums[1], nums[2]])
    });
    (seconds, load)
}

/// Collects a host snapshot (remote_monitor_snapshot).
pub async fn snapshot(
    store: &Store,
    connection_id: &str,
    include_processes: bool,
    process_limit: Option<u64>,
) -> Result<Value, String> {
    let hostname = exec_remote(store, connection_id, "hostname")
        .await
        .unwrap_or_default();
    let uptime = exec_remote(store, connection_id, "uptime")
        .await
        .unwrap_or_default();
    let uname = exec_remote(store, connection_id, "uname -sr")
        .await
        .unwrap_or_default();
    let (uptime_seconds, load_avg) = parse_uptime(&uptime);
    let nproc = exec_remote(store, connection_id, "nproc")
        .await
        .unwrap_or_default();
    let logical_cores = nproc.trim().parse::<u64>().ok();

    let mem = exec_remote(store, connection_id, "free -b")
        .await
        .unwrap_or_default();
    let (total_bytes, used_bytes, available_bytes) = parse_free(&mem);

    let disk = exec_remote(store, connection_id, "df -B1 /")
        .await
        .unwrap_or_default();
    let (disk_total, disk_used, disk_avail) = parse_df(&disk);

    let mut processes = json!([]);
    if include_processes {
        let limit = process_limit.unwrap_or(50);
        let cmd = format!("ps -eo pid,comm,%cpu,%mem,rss,state --sort=-%cpu | head -{limit}");
        if let Ok(out) = exec_remote(store, connection_id, &cmd).await {
            processes = parse_processes(&out);
        }
    }

    let collected_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    Ok(json!({
        "collected_at_ms": collected_at_ms,
        "refresh_hint_ms": 5000,
        "host": {
            "hostname": hostname.trim(),
            "uptime_seconds": uptime_seconds,
            "os": {
                "id": null,
                "name": null,
                "version": null,
                "kernel": uname.trim(),
                "arch": null,
            },
        },
        "cpu": {
            "model": null,
            "logical_cores": logical_cores,
            "load_avg": load_avg,
            "cores": [],
        },
        "memory": {
            "total_bytes": total_bytes,
            "used_bytes": used_bytes,
            "available_bytes": available_bytes,
        },
        "gpus": [],
        "disks": [{
            "mount_point": "/",
            "total_bytes": disk_total,
            "used_bytes": disk_used,
            "available_bytes": disk_avail,
        }],
        "network": {},
        "processes": { "items": processes, "truncated": false },
    }))
}

fn parse_free(text: &str) -> (u64, u64, u64) {
    // "Mem: total used free shared buffers cached"
    let mut total = 0u64;
    let mut used = 0u64;
    let mut available = 0u64;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first().copied() == Some("Mem:") && fields.len() >= 3 {
            total = fields[1].parse().unwrap_or(0);
            used = fields[2].parse().unwrap_or(0);
            available = fields.last().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }
    (total, used, available)
}

fn parse_df(text: &str) -> (u64, u64, u64) {
    // "Filesystem 1K-blocks Used Available Use% Mounted on"
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 {
            let total = fields[1].parse::<u64>().unwrap_or(0) * 1024;
            let used = fields[2].parse::<u64>().unwrap_or(0) * 1024;
            let avail = fields[3].parse::<u64>().unwrap_or(0) * 1024;
            return (total, used, avail);
        }
    }
    (0, 0, 0)
}

fn parse_processes(text: &str) -> Value {
    let mut items: Vec<Value> = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 6 {
            items.push(json!({
                "pid": fields[0].parse::<u64>().unwrap_or(0),
                "name": fields[1],
                "cpu_percent": fields[2].parse::<f64>().unwrap_or(0.0),
                "memory_percent": fields[3].parse::<f64>().unwrap_or(0.0),
                "rss_bytes": fields[4].parse::<u64>().unwrap_or(0),
                "state": fields[5],
            }));
        }
    }
    json!(items)
}

/// Sends a signal to a process (remote_monitor_process_signal).
pub async fn process_signal(
    store: &Store,
    connection_id: &str,
    pid: u64,
    signal: &str,
) -> Result<Value, String> {
    let sig_flag = match signal {
        "term" => "TERM",
        "kill" => "KILL",
        "hup" => "HUP",
        _ => return Err(format!("不支持的信号：{signal}")),
    };
    let command = format!("kill -s {sig_flag} {pid}");
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(json!({
        "ok": true,
        "pid": pid,
        "signal": signal,
        "message": format!("已向进程 {pid} 发送 {sig_flag}。{out}"),
    }))
}

/// Accepts all host keys.
#[derive(Clone)]
struct AcceptAllHostKey;

impl client::Handler for AcceptAllHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_free_parses_mem_line() {
        let text = "              total        used        free      shared  buff/cache   available\nMem:          1000         300         100          10         600         650\n";
        let (total, used, available) = parse_free(text);
        assert_eq!(total, 1000);
        assert_eq!(used, 300);
        assert_eq!(available, 650);
    }

    #[test]
    fn parse_uptime_parses_load() {
        let text = " 10:23:45 up 3 days,  4:12,  2 users,  load average: 0.10, 0.20, 0.30";
        let (seconds, load) = parse_uptime(text);
        eprintln!("seconds={seconds:?} load={load:?}");
        let expected = 3 * 86400 + 4 * 3600 + 12 * 60;
        eprintln!("expected={expected}");
        assert_eq!(seconds, Some(expected));
        assert_eq!(load, Some([0.10, 0.20, 0.30]));
    }

    #[test]
    fn parse_processes_shapes_items() {
        let text =
            "PID COMMAND %CPU %MEM RSS STATE\n1 init 0.1 0.2 1024 S\n2 kthreadd 0.0 0.0 0 I\n";
        let items = parse_processes(text);
        assert_eq!(items.as_array().expect("arr").len(), 2);
        assert_eq!(items[0]["pid"], 1);
        assert_eq!(items[1]["name"], "kthreadd");
    }
}
