//! Scheduled tasks (mxterm parity T008).
//!
//! Persists cron-based task rules in the local SQLite store, parses simple
//! cron expressions (minute/hour/day-of-month/month/day-of-week, "*" and
//! numeric), and runs tasks on demand over a real SSH channel. A periodic
//! scheduler tick (driven by the shell) decides which enabled tasks are due.

use std::time::{SystemTime, UNIX_EPOCH};

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use std::sync::Arc;

use crate::sftp::SshTarget;
use crate::store::Store;

fn now_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("task-{nanos:x}")
}

fn summary_json(
    id: &str,
    request: &serde_json::Value,
    now: &str,
    existing: Option<&serde_json::Value>,
) -> serde_json::Value {
    let name = request
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("task");
    let cron = request
        .get("cron")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("* * * * *");
    let command = request
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let enabled = request
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    serde_json::json!({
        "id": id,
        "name": name,
        "cron": cron,
        "command": command,
        "enabled": enabled,
        "updated_at": now,
        "last_run": existing
            .and_then(|e| e.get("last_run").cloned())
            .unwrap_or(serde_json::Value::Null),
    })
}

/// Lists scheduled tasks for a connection (scheduled_task_list).
pub fn list_tasks(store: &Store, connection_id: &str) -> Result<serde_json::Value, String> {
    let tasks = store.list_tasks().map_err(|e| e.to_string())?;
    let items: Vec<serde_json::Value> = tasks
        .iter()
        .filter(|t| {
            t.get("connection_id").and_then(serde_json::Value::as_str) == Some(connection_id)
        })
        .map(|t| {
            let id = t
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            serde_json::json!({
                "id": id,
                "name": t.get("name").and_then(serde_json::Value::as_str).unwrap_or(""),
                "cron": t.get("cron").and_then(serde_json::Value::as_str).unwrap_or(""),
                "command": t.get("command").and_then(serde_json::Value::as_str).unwrap_or(""),
                "enabled": t.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(false),
                "updated_at": t.get("updated_at").and_then(serde_json::Value::as_str).unwrap_or(""),
                "last_run": t.get("last_run").cloned().unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();
    Ok(serde_json::json!(items))
}

/// Saves a scheduled task (scheduled_task_save).
pub fn save_task(
    store: &mut Store,
    connection_id: &str,
    request: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let now = now_str();
    let existing_id = request.get("id").and_then(serde_json::Value::as_str);
    let existing = existing_id.and_then(|id| store.get_task(id).ok().flatten());
    let id = existing_id.map(|s| s.to_string()).unwrap_or_else(new_id);
    let cron = request
        .get("cron")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("* * * * *");
    validate_cron(cron)?;
    let mut summary = summary_json(&id, request, &now, existing.as_ref());
    summary["connection_id"] = serde_json::json!(connection_id);
    store.put_task(&id, &summary).map_err(|e| e.to_string())?;
    let mut out = summary.clone();
    out["connection_id"] = serde_json::json!(connection_id);
    Ok(out)
}

/// Deletes a scheduled task (scheduled_task_delete).
pub fn delete_task(store: &mut Store, task_id: &str) -> Result<serde_json::Value, String> {
    let _ = store.delete_task(task_id);
    Ok(serde_json::json!({ "ok": true, "message": "任务已删除。", "output": null }))
}

/// Sets a task enabled flag (scheduled_task_set_enabled).
pub fn set_enabled(
    store: &mut Store,
    task_id: &str,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    let mut task = store
        .get_task(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务不存在。".to_string())?;
    task["enabled"] = serde_json::json!(enabled);
    task["updated_at"] = serde_json::json!(now_str());
    store.put_task(task_id, &task).map_err(|e| e.to_string())?;
    Ok(task)
}

/// Validates a 5-field cron expression (simple: "*" or integer per field).
pub fn validate_cron(cron: &str) -> Result<(), String> {
    let fields: Vec<&str> = cron.split_whitespace().collect();
    if fields.len() != 5 {
        return Err("cron 表达式必须为 5 段（分 时 日 月 周）。".to_string());
    }
    for field in fields {
        if field == "*" {
            continue;
        }
        if field.parse::<u32>().is_err() {
            return Err(format!("cron 字段无效：{field}"));
        }
    }
    Ok(())
}

/// Decides whether a cron expression matches the current time (minute 0-59,
/// hour 0-23, day-of-month 1-31, month 1-12, day-of-week 0-6). "*" matches all.
pub fn cron_matches(cron: &str, now_secs: u64) -> bool {
    let fields: Vec<&str> = cron.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    let secs_in_min = now_secs % 60;
    let minutes = (now_secs / 60) % 60;
    let hours = (now_secs / 3600) % 24;
    let days = ((now_secs / 86400) % 31) + 1; // day-of-month 1..31 (approx)
    let months = ((now_secs / 86400 / 31) % 12) + 1; // approx month 1..12
    let weekdays = ((now_secs / 86400) + 4) % 7; // 1970-01-01 was Thursday
    let field_matches = |field: &str, value: u64| -> bool {
        field == "*" || field.parse::<u64>().map(|v| v == value).unwrap_or(false)
    };
    secs_in_min < 5
        && field_matches(fields[0], minutes)
        && field_matches(fields[1], hours)
        && field_matches(fields[2], days)
        && field_matches(fields[3], months)
        && field_matches(fields[4], weekdays)
}

/// Runs a task now over SSH (scheduled_task_run_now). Returns ActionResult.
pub async fn run_now(
    store: &Store,
    connection_id: &str,
    task_id: &str,
) -> Result<serde_json::Value, String> {
    let task = store
        .get_task(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务不存在。".to_string())?;
    let command = task
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
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
    let mut output = String::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } => {
                output.push_str(&String::from_utf8_lossy(&data));
            }
            russh::ChannelMsg::ExtendedData { data, .. } => {
                output.push_str(&String::from_utf8_lossy(&data));
            }
            russh::ChannelMsg::Eof => break,
            _ => {}
        }
    }
    let preview: String = output.chars().take(400).collect();
    Ok(serde_json::json!({
        "ok": true,
        "message": "任务已执行。",
        "output": preview,
    }))
}

/// Accepts all host keys (known-hosts trust is a later row).
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

    fn temp_store() -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!(
            "ssh-task-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("t.db");
        let s = Store::open(&db).expect("store");
        (dir, s)
    }

    #[test]
    fn cron_validation() {
        assert!(validate_cron("* * * * *").is_ok());
        assert!(validate_cron("*/5 * * * *").is_err());
        assert!(validate_cron("0 9 * * 1").is_ok());
        assert!(validate_cron("bad").is_err());
    }

    #[test]
    fn cron_matching() {
        // At secs 0-4 of the minute, minute=30, hour=12.
        let base = 12 * 3600 + 30 * 60;
        assert!(cron_matches("30 12 * * *", base));
        assert!(cron_matches("* * * * *", base));
        assert!(!cron_matches("31 12 * * *", base));
        assert!(!cron_matches("30 11 * * *", base));
    }

    #[test]
    fn task_crud_and_enable() {
        let (dir, mut s) = temp_store();
        let saved = save_task(
            &mut s,
            "c1",
            &serde_json::json!({
                "name": "backup", "cron": "0 2 * * *", "command": "tar czf /tmp/b.tgz /data",
                "enabled": true
            }),
        )
        .expect("save");
        let id = saved["id"].as_str().expect("id").to_string();
        assert_eq!(
            list_tasks(&s, "c1")
                .expect("list")
                .as_array()
                .expect("arr")
                .len(),
            1
        );
        assert_eq!(
            list_tasks(&s, "other")
                .expect("list")
                .as_array()
                .expect("arr")
                .len(),
            0
        );
        set_enabled(&mut s, &id, false).expect("disable");
        let listed = list_tasks(&s, "c1").expect("list");
        assert_eq!(listed[0]["enabled"], false);
        delete_task(&mut s, &id).expect("delete");
        assert_eq!(
            list_tasks(&s, "c1")
                .expect("list")
                .as_array()
                .expect("arr")
                .len(),
            0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
