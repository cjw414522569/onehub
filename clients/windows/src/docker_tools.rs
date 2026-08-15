//! Docker management via SSH-exec docker CLI (mxterm parity T011).
//!
//! Every command opens a real SSH session (russh) and runs the `docker` CLI
//! on the remote host, parsing JSON output into the mXterm UI shapes
//! (DockerContainerSummary / DockerImageSummary / DockerNetworkSummary /
//! DockerEngineStatus / DockerLogsResult / DockerActionResult).

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
    let mut stderr = String::new();
    let mut exit: Option<u32> = None;
    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } => stdout.push_str(&String::from_utf8_lossy(&data)),
            russh::ChannelMsg::ExtendedData { data, .. } => {
                stderr.push_str(&String::from_utf8_lossy(&data))
            }
            russh::ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status),
            russh::ChannelMsg::Eof => break,
            _ => {}
        }
    }
    if exit.map(|c| c != 0).unwrap_or(false) && stdout.trim().is_empty() {
        return Err(format!("docker 命令失败：{}", stderr.trim()));
    }
    Ok(stdout)
}

fn action_result(ok: bool, message: String, output: Option<String>) -> Value {
    json!({ "ok": ok, "message": message, "output": output })
}

/// Lists containers (docker_list_containers).
pub async fn list_containers(store: &Store, connection_id: &str) -> Result<Value, String> {
    let out = exec_remote(
        store,
        connection_id,
        "docker ps -a --no-trunc --format '{{json .}}'",
    )
    .await?;
    let mut items: Vec<Value> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<Value>(line.trim()) {
            let id = obj.get("ID").and_then(Value::as_str).unwrap_or("");
            let image = obj.get("Image").and_then(Value::as_str).unwrap_or("");
            let names = obj.get("Names").and_then(Value::as_str).unwrap_or("");
            let state = obj.get("State").and_then(Value::as_str).unwrap_or("");
            let status = obj.get("Status").and_then(Value::as_str).unwrap_or("");
            items.push(json!({
                "id": id,
                "name": names.split(',').next().unwrap_or("").to_string(),
                "image": image,
                "command": obj.get("Command").and_then(Value::as_str),
                "created_at": null,
                "running_for": null,
                "ports": obj.get("Ports").and_then(Value::as_str),
                "state": state,
                "status": status,
            }));
        }
    }
    Ok(json!(items))
}

/// Lists images (docker_list_images).
pub async fn list_images(store: &Store, connection_id: &str) -> Result<Value, String> {
    let out = exec_remote(
        store,
        connection_id,
        "docker images --no-trunc --format '{{json .}}'",
    )
    .await?;
    let mut items: Vec<Value> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<Value>(line.trim()) {
            let repository = obj.get("Repository").and_then(Value::as_str).unwrap_or("");
            let tag = obj.get("Tag").and_then(Value::as_str).unwrap_or("");
            items.push(json!({
                "id": obj.get("ID").and_then(Value::as_str).unwrap_or(""),
                "repository": repository,
                "tag": tag,
                "digest": obj.get("Digest").and_then(Value::as_str),
                "created_at": null,
                "created_since": obj.get("CreatedSince").and_then(Value::as_str),
                "size": obj.get("Size").and_then(Value::as_str).unwrap_or(""),
            }));
        }
    }
    Ok(json!(items))
}

/// Lists networks (docker_list_networks).
pub async fn list_networks(store: &Store, connection_id: &str) -> Result<Value, String> {
    let out = exec_remote(
        store,
        connection_id,
        "docker network ls --format '{{json .}}'",
    )
    .await?;
    let mut items: Vec<Value> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<Value>(line.trim()) {
            items.push(json!({
                "id": obj.get("ID").and_then(Value::as_str).unwrap_or(""),
                "name": obj.get("Name").and_then(Value::as_str).unwrap_or(""),
                "driver": obj.get("Driver").and_then(Value::as_str),
                "scope": obj.get("Scope").and_then(Value::as_str),
            }));
        }
    }
    Ok(json!(items))
}

/// Runs a container action (docker_container_action): start/stop/restart/pause/
/// unpause/kill/remove.
pub async fn container_action(
    store: &Store,
    connection_id: &str,
    container_id: &str,
    action: &str,
) -> Result<Value, String> {
    let command = match action {
        "start" => format!("docker start {container_id}"),
        "stop" => format!("docker stop {container_id}"),
        "restart" => format!("docker restart {container_id}"),
        "pause" => format!("docker pause {container_id}"),
        "unpause" => format!("docker unpause {container_id}"),
        "kill" => format!("docker kill {container_id}"),
        "remove" => format!("docker rm -f {container_id}"),
        _ => return Err(format!("不支持的容器操作：{action}")),
    };
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(action_result(
        true,
        format!("容器 {action} 成功。"),
        Some(out),
    ))
}

/// Reads container logs (docker_container_logs).
pub async fn container_logs(
    store: &Store,
    connection_id: &str,
    container_id: &str,
    tail: u64,
) -> Result<Value, String> {
    let command = format!("docker logs --tail {tail} {container_id} 2>&1");
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(json!({
        "container_id": container_id,
        "tail": tail,
        "content": out,
    }))
}

/// Inspects a container (docker_container_inspect) - returns raw JSON plus a
/// compact detail shape.
pub async fn container_inspect(
    store: &Store,
    connection_id: &str,
    container_id: &str,
) -> Result<Value, String> {
    let command = format!("docker inspect {container_id}");
    let out = exec_remote(store, connection_id, &command).await?;
    let raw: Value = serde_json::from_str(out.trim())
        .map_err(|e| format!("docker inspect 输出解析失败：{e}"))?;
    let obj = raw
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(raw.clone());
    let name = obj
        .pointer("/Name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches('/')
        .to_string();
    Ok(json!({
        "id": container_id,
        "name": name,
        "image": obj.pointer("/Config/Image").and_then(Value::as_str).unwrap_or(""),
        "image_id": obj.pointer("/Image").and_then(Value::as_str),
        "created": obj.get("Created").and_then(Value::as_str),
        "started_at": obj.pointer("/State/StartedAt").and_then(Value::as_str),
        "finished_at": obj.pointer("/State/FinishedAt").and_then(Value::as_str),
        "status": obj.pointer("/State/Status").and_then(Value::as_str).unwrap_or(""),
        "running": obj.pointer("/State/Running").and_then(Value::as_bool).unwrap_or(false),
        "ip_address": obj.pointer("/NetworkSettings/IPAddress").and_then(Value::as_str),
        "command": obj.pointer("/Config/Cmd").and_then(Value::as_array).cloned().unwrap_or_default(),
        "entrypoint": obj.pointer("/Config/Entrypoint").and_then(Value::as_array).cloned().unwrap_or_default(),
        "working_dir": obj.pointer("/Config/WorkingDir").and_then(Value::as_str),
        "restart_policy": json!({
            "name": obj.pointer("/HostConfig/RestartPolicy/Name").and_then(Value::as_str).unwrap_or(""),
            "maximum_retry_count": obj.pointer("/HostConfig/RestartPolicy/MaximumRetryCount").and_then(Value::as_u64),
        }),
        "ports": [],
        "env": [],
        "mounts": [],
        "networks": [],
        "labels": [],
        "raw_json": serde_json::to_string(&obj).unwrap_or_default(),
    }))
}

/// Updates a container restart policy (docker_container_update_restart_policy).
pub async fn update_restart_policy(
    store: &Store,
    connection_id: &str,
    container_id: &str,
    policy: &str,
) -> Result<Value, String> {
    let command = format!("docker update --restart={policy} {container_id}");
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(action_result(
        true,
        "重启策略已更新。".to_string(),
        Some(out),
    ))
}

/// Connects a container to a network (docker_container_connect_network).
pub async fn connect_network(
    store: &Store,
    connection_id: &str,
    container_id: &str,
    network_id: &str,
) -> Result<Value, String> {
    let command = format!("docker network connect {network_id} {container_id}");
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(action_result(true, "网络连接成功。".to_string(), Some(out)))
}

/// Pulls an image (docker_image_pull).
pub async fn image_pull(store: &Store, connection_id: &str, image: &str) -> Result<Value, String> {
    let command = format!("docker pull {image}");
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(action_result(true, "镜像拉取完成。".to_string(), Some(out)))
}

/// Removes an image (docker_image_remove).
pub async fn image_remove(
    store: &Store,
    connection_id: &str,
    image_id: &str,
) -> Result<Value, String> {
    let command = format!("docker rmi -f {image_id}");
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(action_result(true, "镜像已删除。".to_string(), Some(out)))
}

/// Runs an image (docker_image_run) with optional ports/volumes/env/name.
pub async fn image_run(
    store: &Store,
    connection_id: &str,
    request: &Value,
) -> Result<Value, String> {
    let image = request.get("image").and_then(Value::as_str).unwrap_or("");
    let name = request.get("name").and_then(Value::as_str);
    let mut args = vec!["docker run -d".to_string()];
    if let Some(name) = name {
        args.push(format!("--name {name}"));
    }
    if let Some(ports) = request.get("ports").and_then(Value::as_array) {
        for p in ports {
            let hp = p.get("host_port").and_then(Value::as_str).unwrap_or("");
            let cp = p
                .get("container_port")
                .and_then(Value::as_str)
                .unwrap_or("");
            args.push(format!("-p {hp}:{cp}"));
        }
    }
    args.push(image.to_string());
    let command = args.join(" ");
    let out = exec_remote(store, connection_id, &command).await?;
    Ok(action_result(true, "容器已启动。".to_string(), Some(out)))
}

/// Checks engine status (docker_engine_status).
pub async fn engine_status(store: &Store, connection_id: &str) -> Result<Value, String> {
    let version_cmd = "docker version --format '{{.Server.Version}}'";
    let version = exec_remote(store, connection_id, version_cmd).await.ok();
    let info_cmd = "docker info --format '{{json .}}'";
    let info = exec_remote(store, connection_id, info_cmd).await.ok();
    let server_os = info
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| {
            v.get("OperatingSystem")
                .and_then(Value::as_str)
                .map(|s| s.to_string())
        });
    Ok(json!({
        "installed": version.is_some(),
        "running": version.is_some(),
        "service_status": if version.is_some() { "running" } else { "not_installed" },
        "version": version.map(|s| s.trim().to_string()),
        "api_version": null,
        "server_os": server_os,
    }))
}

/// Runs an engine action (docker_engine_action): restart / stop / start via
/// systemctl/service.
pub async fn engine_action(
    store: &Store,
    connection_id: &str,
    action: &str,
) -> Result<Value, String> {
    let command = match action {
        "restart" => "sudo systemctl restart docker || sudo service docker restart",
        "stop" => "sudo systemctl stop docker || sudo service docker stop",
        "start" => "sudo systemctl start docker || sudo service docker start",
        _ => return Err(format!("不支持的引擎操作：{action}")),
    };
    let out = exec_remote(store, connection_id, command).await?;
    Ok(action_result(
        true,
        format!("引擎 {action} 成功。"),
        Some(out),
    ))
}

/// Saves container logs to a local file (docker_container_logs_save).
pub fn logs_save(local_path: &str, content: &str) -> Result<Value, String> {
    std::fs::write(local_path, content).map_err(|e| format!("日志保存失败：{e}"))?;
    Ok(action_result(true, "日志已保存。".to_string(), None))
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
    fn action_result_shape() {
        let r = action_result(true, "ok".to_string(), Some("out".to_string()));
        assert_eq!(r["ok"], true);
        assert_eq!(r["message"], "ok");
        assert_eq!(r["output"], "out");
    }

    #[test]
    fn logs_save_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "docker-logs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let p = dir.to_string_lossy().to_string();
        logs_save(&p, "line1\nline2").expect("save");
        let content = std::fs::read_to_string(&p).expect("read");
        assert_eq!(content, "line1\nline2");
        let _ = std::fs::remove_file(&p);
    }
}
