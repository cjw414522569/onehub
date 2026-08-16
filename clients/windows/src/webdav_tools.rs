//! WebDAV sync (mxterm parity T017).
//!
//! Settings persist in the local SQLite store; the WebDAV password is
//! encrypted at rest with the local secret. Snapshots use a small
//! mxterm-compatible bundle: a plaintext manifest plus plaintext data.json and
//! an optional AES-256-GCM secrets envelope keyed by an Argon2id-derived sync
//! password. HTTP is served through the blocking `ureq` client.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::store::Store;

const WEBDAV_SETTINGS_KEY: &str = "webdav.default";
const DEFAULT_REMOTE_ROOT: &str = "mxterm-sync";
const DEFAULT_PROFILE: &str = "default";
const PROTOCOL_VERSION: u64 = 1;

fn now_ts() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_snapshot_id() -> String {
    format!("snap-{}-{:x}", std::process::id(), now_ms())
}

// ---- settings ----

fn default_settings() -> Value {
    json!({
        "enabled": false,
        "base_url": "",
        "username": Value::Null,
        "password_encrypted": Value::Null,
        "remote_root": DEFAULT_REMOTE_ROOT,
        "profile": DEFAULT_PROFILE,
        "last_sync_at": Value::Null,
        "last_snapshot_id": Value::Null,
        "last_remote_device_name": Value::Null,
        "last_error": Value::Null,
        "updated_at": now_ts(),
    })
}

fn load_settings(store: &Store) -> Result<Value, String> {
    Ok(store
        .get_app_setting(WEBDAV_SETTINGS_KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(default_settings))
}

/// webdav_settings_get.
pub fn settings_get(store: &Store) -> Result<Value, String> {
    Ok(settings_output(load_settings(store)?))
}

/// webdav_settings_save: normalizes + persists settings; the WebDAV password
/// is encrypted at rest when `password_touched` is set.
pub fn settings_save(store: &mut Store, request: &Value) -> Result<Value, String> {
    let existing = load_settings(store)?;
    let enabled = request
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let base_url = request
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let username = request
        .get("username")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let remote_root = request
        .get("remote_root")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_REMOTE_ROOT.to_string());
    let profile = request
        .get("profile")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_PROFILE.to_string());
    if !base_url.is_empty() {
        validate_base_url(&base_url)?;
    }
    let password_touched = request
        .get("password_touched")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut password_encrypted = existing
        .get("password_encrypted")
        .cloned()
        .unwrap_or(Value::Null);
    if password_touched {
        let password = request
            .get("password")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_default();
        if password.is_empty() {
            password_encrypted = Value::Null;
        } else {
            let key = local_secret_key(store)?;
            let enc = encrypt_password(&key, password.as_bytes())?;
            password_encrypted = json!(enc);
        }
    }
    let mut settings = json!({
        "enabled": enabled,
        "base_url": base_url,
        "username": username,
        "password_encrypted": password_encrypted,
        "remote_root": remote_root,
        "profile": profile,
        "last_sync_at": existing.get("last_sync_at").cloned().unwrap_or(Value::Null),
        "last_snapshot_id": existing.get("last_snapshot_id").cloned().unwrap_or(Value::Null),
        "last_remote_device_name": existing.get("last_remote_device_name").cloned().unwrap_or(Value::Null),
        "last_error": Value::Null,
        "updated_at": now_ts(),
    });
    store
        .put_app_setting(WEBDAV_SETTINGS_KEY, &settings)
        .map_err(|e| e.to_string())?;
    if !settings["enabled"].as_bool().unwrap_or(false)
        || settings["base_url"].as_str().unwrap_or("").is_empty()
    {
        settings["last_error"] = Value::Null;
    }
    Ok(settings_output(settings))
}

fn validate_base_url(base_url: &str) -> Result<(), String> {
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err("WebDAV 地址必须以 http:// 或 https:// 开头。".to_string());
    }
    Ok(())
}

fn settings_output(settings: Value) -> Value {
    let password_saved = settings
        .get("password_encrypted")
        .and_then(Value::as_str)
        .is_some();
    json!({
        "enabled": settings["enabled"],
        "base_url": settings["base_url"],
        "username": settings.get("username").cloned().unwrap_or(Value::Null),
        "password_saved": password_saved,
        "remote_root": settings["remote_root"],
        "profile": settings["profile"],
        "last_sync_at": settings.get("last_sync_at").cloned().unwrap_or(Value::Null),
        "last_snapshot_id": settings.get("last_snapshot_id").cloned().unwrap_or(Value::Null),
        "last_remote_device_name": settings.get("last_remote_device_name").cloned().unwrap_or(Value::Null),
        "last_error": settings.get("last_error").cloned().unwrap_or(Value::Null),
        "updated_at": settings["updated_at"],
    })
}

fn settings_for_request(
    store: &Store,
    request: Option<&Value>,
) -> Result<(Value, Option<String>), String> {
    match request {
        Some(input) => {
            // Build a temporary settings snapshot from the input without
            // persisting; password comes from the input.
            let enabled = input
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let base_url = input
                .get("base_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if !base_url.is_empty() {
                validate_base_url(&base_url)?;
            }
            let username = input
                .get("username")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            let password = input
                .get("password")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            let settings = json!({
                "enabled": enabled,
                "base_url": base_url,
                "username": username,
                "remote_root": input.get("remote_root").and_then(Value::as_str).unwrap_or(DEFAULT_REMOTE_ROOT).to_string(),
                "profile": input.get("profile").and_then(Value::as_str).unwrap_or(DEFAULT_PROFILE).to_string(),
            });
            Ok((settings, password))
        }
        None => {
            let settings = load_settings(store)?;
            let password = decrypt_stored_password(store, &settings)?;
            Ok((settings, password))
        }
    }
}

fn decrypt_stored_password(store: &Store, settings: &Value) -> Result<Option<String>, String> {
    let enc = settings.get("password_encrypted").and_then(Value::as_str);
    let Some(enc) = enc else { return Ok(None) };
    let key = existing_secret_key(store).ok_or_else(|| "WebDAV 密码密钥缺失。".to_string())?;
    let plain = decrypt_password(&key, enc)?;
    Ok(Some(String::from_utf8_lossy(&plain).to_string()))
}

fn existing_secret_key(store: &Store) -> Option<[u8; 32]> {
    let existing = store.get_app_secret("webdav_pw_enc").ok()??;
    let bytes = b64_decode(&existing)?;
    if bytes.len() == 32 {
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Some(key)
    } else {
        None
    }
}

fn record_sync_success(
    store: &mut Store,
    settings: &Value,
    snapshot_id: &str,
    device_name: &str,
) -> Result<Value, String> {
    let mut updated = settings.clone();
    updated["last_sync_at"] = json!(now_ts());
    updated["last_snapshot_id"] = json!(snapshot_id);
    updated["last_remote_device_name"] = json!(device_name);
    updated["last_error"] = Value::Null;
    updated["updated_at"] = json!(now_ts());
    store
        .put_app_setting(WEBDAV_SETTINGS_KEY, &updated)
        .map_err(|e| e.to_string())?;
    Ok(settings_output(updated))
}

// ---- local password encryption ----

fn local_secret_key(store: &mut Store) -> Result<[u8; 32], String> {
    if let Some(existing) = store
        .get_app_secret("webdav_pw_enc")
        .map_err(|e| e.to_string())?
    {
        if let Some(bytes) = b64_decode(&existing) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }
    }
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    store
        .put_app_secret("webdav_pw_enc", &b64_encode(&key))
        .map_err(|e| e.to_string())?;
    Ok(key)
}

fn encrypt_password(key: &[u8; 32], plaintext: &[u8]) -> Result<String, String> {
    use aes_gcm::aead::{Aead, KeyInit, Nonce};
    use aes_gcm::{Aes256Gcm, Key};
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let encrypted = cipher
        .encrypt(Nonce::<Aes256Gcm>::from_slice(&nonce), plaintext)
        .map_err(|e| e.to_string())?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&encrypted);
    Ok(b64_encode(&out))
}

fn decrypt_password(key: &[u8; 32], blob: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit, Nonce};
    use aes_gcm::{Aes256Gcm, Key};
    let data = b64_decode(blob).ok_or_else(|| "WebDAV 密码密文无效。".to_string())?;
    if data.len() < 12 {
        return Err("WebDAV 密码密文过短。".to_string());
    }
    let (nonce, ciphertext) = data.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::<Aes256Gcm>::from_slice(nonce), ciphertext)
        .map_err(|_| "WebDAV 密码解密失败。".to_string())
}

fn b64_encode(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}
// ---- HTTP client (ureq, blocking) ----

fn basic_auth(username: Option<&str>, password: Option<&str>) -> Option<String> {
    let username = username.unwrap_or("");
    if username.is_empty() {
        return None;
    }
    let password = password.unwrap_or("");
    use base64::Engine as _;
    let raw = format!("{username}:{password}");
    Some(format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    ))
}

fn webdav_get(
    url: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<Option<(u16, String)>, String> {
    let mut request = ureq::get(url);
    if let Some(auth) = basic_auth(username, password) {
        request = request.set("Authorization", &auth);
    }
    match request.call() {
        Ok(response) => {
            let status = response.status();
            let text = response.into_string().unwrap_or_default();
            Ok(Some((status, text)))
        }
        Err(ureq::Error::Status(code, response)) => {
            if code == 404 {
                Ok(None)
            } else {
                Err(format!(
                    "WebDAV GET 失败（{code}）：{}",
                    response.into_string().unwrap_or_default()
                ))
            }
        }
        Err(e) => Err(format!("WebDAV GET 失败：{e}")),
    }
}

fn webdav_put(
    url: &str,
    username: Option<&str>,
    password: Option<&str>,
    body: &str,
) -> Result<(), String> {
    let mut request = ureq::request("PUT", url).set("Content-Type", "application/octet-stream");
    if let Some(auth) = basic_auth(username, password) {
        request = request.set("Authorization", &auth);
    }
    match request.send_string(body) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, response)) => Err(format!(
            "WebDAV PUT 失败（{code}）：{}",
            response.into_string().unwrap_or_default()
        )),
        Err(e) => Err(format!("WebDAV PUT 失败：{e}")),
    }
}

fn webdav_mkcol(url: &str, username: Option<&str>, password: Option<&str>) -> Result<u16, String> {
    let mut request = ureq::request("MKCOL", url);
    if let Some(auth) = basic_auth(username, password) {
        request = request.set("Authorization", &auth);
    }
    match request.call() {
        Ok(response) => Ok(response.status()),
        Err(ureq::Error::Status(code, _)) => Ok(code),
        Err(e) => Err(format!("WebDAV MKCOL 失败：{e}")),
    }
}

/// Ensures the remote collection exists (PROPFIND, then MKCOL on 404),
/// mirroring mxterm's `ensure_collection`.
fn ensure_collection(
    base_url: &str,
    username: Option<&str>,
    password: Option<&str>,
    remote_root: &str,
    profile: &str,
) -> Result<(), String> {
    let collection = format!(
        "{}/{}/{}",
        base_url.trim_end_matches('/'),
        remote_root.trim_matches('/'),
        profile.trim_matches('/')
    );
    match webdav_propfind(&collection, username, password)? {
        Some(_) => Ok(()),
        None => {
            let status = webdav_mkcol(&collection, username, password)?;
            if status == 201 || status == 405 || status == 301 {
                Ok(())
            } else {
                Err(format!("WebDAV 集合创建失败（{status}）。"))
            }
        }
    }
}

fn webdav_propfind(
    url: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<Option<u16>, String> {
    let mut request = ureq::request("PROPFIND", url).set("Depth", "0");
    if let Some(auth) = basic_auth(username, password) {
        request = request.set("Authorization", &auth);
    }
    match request.call() {
        Ok(response) => Ok(Some(response.status())),
        Err(ureq::Error::Status(code, _)) => {
            if code == 404 {
                Ok(None)
            } else {
                Err(format!("WebDAV PROPFIND 失败（{code}）。"))
            }
        }
        Err(e) => Err(format!("WebDAV PROPFIND 失败：{e}")),
    }
}

fn remote_url(settings: &Value, artifact: &str) -> String {
    let base_url = settings["base_url"].as_str().unwrap_or("");
    let remote_root = settings["remote_root"]
        .as_str()
        .unwrap_or(DEFAULT_REMOTE_ROOT);
    let profile = settings["profile"].as_str().unwrap_or(DEFAULT_PROFILE);
    format!(
        "{}/{}/{}/{}",
        base_url.trim_end_matches('/'),
        remote_root.trim_matches('/'),
        profile.trim_matches('/'),
        artifact
    )
}

fn ensure_enabled(settings: &Value) -> Result<(), String> {
    if !settings["enabled"].as_bool().unwrap_or(false) {
        return Err("请先启用 WebDAV 同步。".to_string());
    }
    if settings["base_url"].as_str().unwrap_or("").is_empty() {
        return Err("请先配置 WebDAV 地址。".to_string());
    }
    Ok(())
}

fn credentials_for<'a, 'b>(
    settings: &'a Value,
    password: Option<&'b str>,
) -> (Option<&'a str>, Option<&'b str>) {
    let username = settings.get("username").and_then(Value::as_str);
    (username, password)
}

// ---- public commands ----

/// webdav_test_connection: verifies the server is reachable and the remote
/// collection can be ensured.
pub fn test_connection(store: &Store, request: Option<&Value>) -> Result<Value, String> {
    let (settings, password) = settings_for_request(store, request)?;
    ensure_enabled(&settings)?;
    let (username, password) = credentials_for(&settings, password.as_deref());
    ensure_collection(
        settings["base_url"].as_str().unwrap_or(""),
        username,
        password,
        settings["remote_root"]
            .as_str()
            .unwrap_or(DEFAULT_REMOTE_ROOT),
        settings["profile"].as_str().unwrap_or(DEFAULT_PROFILE),
    )?;
    Ok(json!({ "ok": true, "message": "WebDAV 连接正常。" }))
}

/// webdav_fetch_remote_info: reads the remote manifest (if any).
pub fn fetch_remote_info(store: &Store) -> Result<Value, String> {
    let settings = load_settings(store)?;
    ensure_enabled(&settings)?;
    let password = decrypt_stored_password(store, &settings)?;
    let (username, password) = credentials_for(&settings, password.as_deref());
    let manifest_url = remote_url(&settings, "manifest.json");
    let manifest = match webdav_get(&manifest_url, username, password)? {
        Some((_, text)) => {
            serde_json::from_str::<Value>(&text).map_err(|e| format!("远端 manifest 无效：{e}"))?
        }
        None => {
            return Ok(json!({
                "exists": false,
                "compatible": false,
                "snapshot_id": null,
                "device_name": null,
                "created_at": null,
                "protocol_version": null,
                "data_size": null,
                "secrets_size": null,
            }));
        }
    };
    let protocol_version = manifest.get("protocol_version").and_then(Value::as_u64);
    let compatible = protocol_version == Some(PROTOCOL_VERSION)
        && manifest
            .get("snapshot_id")
            .and_then(Value::as_str)
            .is_some();
    Ok(json!({
        "exists": true,
        "compatible": compatible,
        "snapshot_id": manifest.get("snapshot_id").cloned().unwrap_or(Value::Null),
        "device_name": manifest.get("device_name").cloned().unwrap_or(Value::Null),
        "created_at": manifest.get("created_at").cloned().unwrap_or(Value::Null),
        "protocol_version": protocol_version,
        "data_size": manifest.pointer("/artifacts/data.json/size").cloned().unwrap_or(Value::Null),
        "secrets_size": manifest.pointer("/artifacts/secrets.bin/size").cloned().unwrap_or(Value::Null),
    }))
}

/// webdav_upload_snapshot: exports local connections/credentials, PUTs
/// data.json (+ optional secrets.bin), then manifest.json last.
pub fn upload_snapshot(store: &mut Store, request: &Value) -> Result<Value, String> {
    let settings = load_settings(store)?;
    ensure_enabled(&settings)?;
    let password = decrypt_stored_password(store, &settings)?;
    let (username, password) = credentials_for(&settings, password.as_deref());

    let sync_password = request
        .get("sync_password")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let device_id = request
        .get("device_id")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "local-device".to_string());
    let device_name = request
        .get("device_name")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .unwrap_or_else(default_device_name);

    let (manifest, data_json, secrets_bin) = build_snapshot(
        store,
        &settings,
        &device_id,
        &device_name,
        sync_password.as_deref(),
    )?;

    ensure_collection(
        settings["base_url"].as_str().unwrap_or(""),
        username,
        password,
        settings["remote_root"]
            .as_str()
            .unwrap_or(DEFAULT_REMOTE_ROOT),
        settings["profile"].as_str().unwrap_or(DEFAULT_PROFILE),
    )?;
    let base = remote_url(&settings, "");
    let base = base.trim_end_matches('/');
    webdav_put(&format!("{base}/data.json"), username, password, &data_json)?;
    if let Some(secrets) = &secrets_bin {
        webdav_put(&format!("{base}/secrets.bin"), username, password, secrets)?;
    }
    webdav_put(
        &format!("{base}/manifest.json"),
        username,
        password,
        &manifest,
    )?;

    let snapshot_id = manifest_parse_id(&manifest);
    let created_at = manifest_parse_created(&manifest);
    record_sync_success(store, &settings, &snapshot_id, &device_name)?;
    Ok(json!({
        "snapshot_id": snapshot_id,
        "device_name": device_name,
        "created_at": created_at,
        "uploaded": true,
        "downloaded": false,
        "secrets_skipped": false,
    }))
}

/// webdav_download_snapshot: GETs the remote bundle, decrypts secrets with the
/// sync password, and imports connections/credentials into the local store.
pub fn download_snapshot(store: &mut Store, request: &Value) -> Result<Value, String> {
    let settings = load_settings(store)?;
    ensure_enabled(&settings)?;
    let password = decrypt_stored_password(store, &settings)?;
    let (username, password) = credentials_for(&settings, password.as_deref());

    let base = remote_url(&settings, "").trim_end_matches('/').to_string();
    let manifest_text = webdav_get(&format!("{base}/manifest.json"), username, password)?
        .map(|(_, text)| text)
        .ok_or_else(|| "远端没有可下载的快照。".to_string())?;
    let manifest: Value =
        serde_json::from_str(&manifest_text).map_err(|e| format!("远端 manifest 无效：{e}"))?;
    if manifest.get("protocol_version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err("远端快照协议版本不兼容。".to_string());
    }
    let snapshot_id = manifest_parse_id(&manifest_text);
    let device_name = manifest
        .get("device_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let created_at = manifest_parse_created(&manifest_text);

    let data_text = webdav_get(&format!("{base}/data.json"), username, password)?
        .map(|(_, text)| text)
        .ok_or_else(|| "远端缺少 data.json。".to_string())?;
    let data: Value =
        serde_json::from_str(&data_text).map_err(|e| format!("远端 data.json 无效：{e}"))?;

    let mut secrets_map = Value::Null;
    let mut secrets_skipped = false;
    if let Some((_, secrets_text)) = webdav_get(&format!("{base}/secrets.bin"), username, password)?
    {
        let sync_password = request
            .get("sync_password")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if sync_password.is_empty() {
            secrets_skipped = true;
        } else {
            secrets_map = decrypt_secrets(&sync_password, &secrets_text)?;
        }
    }

    import_snapshot(store, &data, secrets_map)?;
    record_sync_success(store, &settings, &snapshot_id, &device_name)?;
    Ok(json!({
        "snapshot_id": snapshot_id,
        "device_name": device_name,
        "created_at": created_at,
        "uploaded": false,
        "downloaded": true,
        "secrets_skipped": secrets_skipped,
    }))
}

fn manifest_parse_id(manifest_text: &str) -> String {
    serde_json::from_str::<Value>(manifest_text)
        .ok()
        .and_then(|v| {
            v.get("snapshot_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn manifest_parse_created(manifest_text: &str) -> String {
    serde_json::from_str::<Value>(manifest_text)
        .ok()
        .and_then(|v| {
            v.get("created_at")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn default_device_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "local-device".to_string())
}
// ---- snapshot bundle ----

fn build_snapshot(
    store: &Store,
    _settings: &Value,
    device_id: &str,
    device_name: &str,
    sync_password: Option<&str>,
) -> Result<(String, String, Option<String>), String> {
    let connections = store.list_connections().map_err(|e| e.to_string())?;
    let credentials = store.list_credentials().map_err(|e| e.to_string())?;
    let mut data_connections: Vec<Value> = Vec::new();
    let mut data_credentials: Vec<Value> = Vec::new();
    let mut secrets: serde_json::Map<String, Value> = serde_json::Map::new();
    for connection in connections {
        let id = connection
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut entry = connection.clone();
        let mut secret_entry = serde_json::Map::new();
        for field in ["password", "private_key_passphrase"] {
            if let Some(value) = entry.get(field) {
                if !value.is_null() {
                    secret_entry.insert(field.to_string(), value.clone());
                }
            }
            entry[field] = Value::Null;
        }
        if !secret_entry.is_empty() {
            secrets.insert(format!("connection:{id}"), Value::Object(secret_entry));
        }
        data_connections.push(entry);
    }
    for credential in credentials {
        let id = credential
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut entry = credential.clone();
        let mut secret_entry = serde_json::Map::new();
        for field in ["password", "private_key_passphrase"] {
            if let Some(value) = entry.get(field) {
                if !value.is_null() {
                    secret_entry.insert(field.to_string(), value.clone());
                }
            }
            entry[field] = Value::Null;
        }
        if !secret_entry.is_empty() {
            secrets.insert(format!("credential:{id}"), Value::Object(secret_entry));
        }
        data_credentials.push(entry);
    }
    let app_settings = store.list_app_setting_pairs().map_err(|e| e.to_string())?;
    let data = json!({
        "connections": data_connections,
        "credentials": data_credentials,
        "settings": app_settings,
    });
    let data_json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    let mut secrets_bin: Option<String> = None;
    if !secrets.is_empty() {
        let sync_password = sync_password.unwrap_or("");
        if sync_password.is_empty() {
            return Err("同步包含已保存 SSH 密码或口令，请输入同步主密码。".to_string());
        }
        secrets_bin = Some(encrypt_secrets(sync_password, &Value::Object(secrets))?);
    }
    let snapshot_id = new_snapshot_id();
    let created_at = now_ts();
    let mut artifacts = serde_json::Map::new();
    artifacts.insert("data.json".to_string(), json!({ "size": data_json.len() }));
    if let Some(bin) = &secrets_bin {
        artifacts.insert("secrets.bin".to_string(), json!({ "size": bin.len() }));
    }
    let manifest = json!({
        "format": "mxterm-sync",
        "protocol_version": PROTOCOL_VERSION,
        "snapshot_id": snapshot_id,
        "device_id": device_id,
        "device_name": device_name,
        "created_at": created_at,
        "artifacts": Value::Object(artifacts),
    });
    Ok((
        serde_json::to_string(&manifest).map_err(|e| e.to_string())?,
        data_json,
        secrets_bin,
    ))
}

fn derive_sync_key(sync_password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(sync_password.as_bytes(), salt, &mut out)
        .map_err(|e| format!("密钥派生失败：{e}"))?;
    Ok(out)
}

fn encrypt_secrets(sync_password: &str, secrets: &Value) -> Result<String, String> {
    use aes_gcm::aead::{Aead, KeyInit, Nonce};
    use aes_gcm::{Aes256Gcm, Key};
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| e.to_string())?;
    let key = derive_sync_key(sync_password, &salt)?;
    let plain = serde_json::to_vec(secrets).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let encrypted = cipher
        .encrypt(Nonce::<Aes256Gcm>::from_slice(&nonce), plain.as_slice())
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&json!({
        "salt": b64_encode(&salt),
        "nonce": b64_encode(&nonce),
        "ciphertext": b64_encode(&encrypted),
    }))
    .map_err(|e| e.to_string())
}

fn decrypt_secrets(sync_password: &str, blob: &str) -> Result<Value, String> {
    use aes_gcm::aead::{Aead, KeyInit, Nonce};
    use aes_gcm::{Aes256Gcm, Key};
    let envelope: Value =
        serde_json::from_str(blob).map_err(|e| format!("secrets.bin 无效：{e}"))?;
    let salt = b64_decode(envelope.get("salt").and_then(Value::as_str).unwrap_or(""))
        .ok_or_else(|| "secrets salt 无效。".to_string())?;
    let nonce = b64_decode(envelope.get("nonce").and_then(Value::as_str).unwrap_or(""))
        .ok_or_else(|| "secrets nonce 无效。".to_string())?;
    let ciphertext = b64_decode(
        envelope
            .get("ciphertext")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
    .ok_or_else(|| "secrets 密文无效。".to_string())?;
    let key = derive_sync_key(sync_password, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let plain = cipher
        .decrypt(
            Nonce::<Aes256Gcm>::from_slice(&nonce),
            ciphertext.as_slice(),
        )
        .map_err(|_| "同步密码错误或快照损坏。".to_string())?;
    serde_json::from_slice(&plain).map_err(|e| format!("secrets 数据无效：{e}"))
}

fn import_snapshot(store: &mut Store, data: &Value, secrets_map: Value) -> Result<(), String> {
    let connections = data
        .get("connections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for connection in connections {
        let id = connection
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut profile = connection.clone();
        let key = format!("connection:{id}");
        if let Some(secrets) = secrets_map.get(key.as_str()) {
            if let Some(value) = secrets.get("password") {
                profile["password"] = value.clone();
            }
            if let Some(value) = secrets.get("private_key_passphrase") {
                profile["private_key_passphrase"] = value.clone();
            }
        }
        let _ = store.upsert_connection(&profile);
    }
    let credentials = data
        .get("credentials")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for credential in credentials {
        let id = credential
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut profile = credential.clone();
        let key = format!("credential:{id}");
        if let Some(secrets) = secrets_map.get(key.as_str()) {
            if let Some(value) = secrets.get("password") {
                profile["password"] = value.clone();
            }
            if let Some(value) = secrets.get("private_key_passphrase") {
                profile["private_key_passphrase"] = value.clone();
            }
        }
        let _ = store.upsert_credential(&profile);
    }
    let settings = data
        .get("settings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for setting in settings {
        let key = setting
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if key.is_empty() {
            continue;
        }
        let value = setting.get("value").cloned().unwrap_or(Value::Null);
        let _ = store.put_app_setting(&key, &value);
    }
    Ok(())
}

/// Starts a tiny in-memory WebDAV-like server used by the unit test and the
/// --webdav-check end-to-end check. Returns (port, shared file map).
pub fn fake_webdav_server_for_checks() -> (
    u16,
    std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<std::string::String, std::string::String>>,
    >,
) {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    let server = tiny_http::Server::http("127.0.0.1:0").expect("fake webdav server");
    let port = server.server_addr().to_ip().expect("server ip").port();
    let files: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let files_for_thread = files.clone();
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let method = request.method().clone();
            let mut body = String::new();
            if matches!(method, tiny_http::Method::Put) {
                let mut buf = Vec::new();
                let _ = request.as_reader().read_to_end(&mut buf);
                body = String::from_utf8_lossy(&buf).to_string();
            }
            let response = match &method {
                tiny_http::Method::Options => {
                    tiny_http::Response::from_string("").with_status_code(200)
                }
                tiny_http::Method::Put => {
                    files_for_thread
                        .lock()
                        .expect("files lock")
                        .insert(url, body);
                    tiny_http::Response::from_string("").with_status_code(201)
                }
                tiny_http::Method::Get => {
                    match files_for_thread.lock().expect("files lock").get(&url) {
                        Some(text) => {
                            tiny_http::Response::from_string(text.clone()).with_status_code(200)
                        }
                        None => tiny_http::Response::from_string("not found").with_status_code(404),
                    }
                }
                tiny_http::Method::NonStandard(method_name) if method_name == "PROPFIND" => {
                    tiny_http::Response::from_string("").with_status_code(207)
                }
                tiny_http::Method::NonStandard(method_name) if method_name == "MKCOL" => {
                    tiny_http::Response::from_string("").with_status_code(201)
                }
                _ => tiny_http::Response::from_string("").with_status_code(200),
            };
            let _ = request.respond(response);
        }
    });
    (port, files)
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_password_roundtrip_encrypted() {
        let dir = std::env::temp_dir().join(format!("webdav-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("w.db");
        let mut store = Store::open(&db).expect("store");
        let saved = settings_save(
            &mut store,
            &json!({
                "enabled": true,
                "base_url": "http://127.0.0.1:8080/dav",
                "username": "user",
                "password": "secret-pw",
                "password_touched": true,
                "remote_root": "mxterm-sync",
                "profile": "default",
            }),
        )
        .expect("save");
        assert_eq!(saved["password_saved"].as_bool(), Some(true));
        assert!(saved.get("password_encrypted").is_none());
        let got = settings_get(&store).expect("get");
        assert_eq!(got["username"], "user");
        assert_eq!(got["password_saved"].as_bool(), Some(true));
        // blank touched password clears the saved flag
        let saved2 = settings_save(
            &mut store,
            &json!({
                "enabled": true,
                "base_url": "http://127.0.0.1:8080/dav",
                "username": "user",
                "password": "",
                "password_touched": true,
                "remote_root": "mxterm-sync",
                "profile": "default",
            }),
        )
        .expect("save2");
        assert_eq!(saved2["password_saved"].as_bool(), Some(false));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn secrets_envelope_roundtrip() {
        let secrets = json!({ "connection:c1": { "password": "p1" } });
        let blob = encrypt_secrets("sync-pass", &secrets).expect("encrypt");
        let decrypted = decrypt_secrets("sync-pass", &blob).expect("decrypt");
        assert_eq!(decrypted["connection:c1"]["password"], "p1");
        assert!(decrypt_secrets("wrong-pass", &blob).is_err());
    }

    #[test]
    fn snapshot_requires_sync_password_when_secrets_exist() {
        let dir = std::env::temp_dir().join(format!("webdav-test2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("w2.db");
        let mut store = Store::open(&db).expect("store");
        store
            .upsert_credential(&json!({
                "id": "cred-1",
                "name": "cred",
                "kind": "password",
                "username": "u",
                "password": "pw",
            }))
            .expect("upsert");
        let settings = default_settings();
        let result = build_snapshot(&store, &settings, "dev", "dev-name", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("同步主密码"));
        let result = build_snapshot(&store, &settings, "dev", "dev-name", Some("sync-pass"));
        let (manifest, data_json, secrets_bin) = result.expect("build");
        assert!(manifest.contains("\"protocol_version\":1"));
        assert!(data_json.contains("\"cred-1\""));
        assert!(secrets_bin.is_some());
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn full_webdav_roundtrip_over_http() {
        let (port, _files) = fake_webdav_server_for_checks();
        let base = format!("http://127.0.0.1:{port}/dav");
        let dir = std::env::temp_dir().join(format!("webdav-e2e-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("w.db");
        let mut store = Store::open(&db).expect("store");
        store
            .upsert_connection(&json!({
                "id": "conn-1",
                "name": "host",
                "host": "10.0.0.1",
                "port": 22,
                "username": "root",
                "password": "secret",
            }))
            .expect("upsert connection");
        store
            .upsert_credential(&json!({
                "id": "cred-1",
                "name": "cred",
                "kind": "password",
                "username": "u",
                "password": "pw-secret",
            }))
            .expect("upsert credential");
        let saved = settings_save(
            &mut store,
            &json!({
                "enabled": true,
                "base_url": base.clone(),
                "username": "webdav-user",
                "password": "webdav-pw",
                "password_touched": true,
                "remote_root": "mxterm-sync",
                "profile": "default",
            }),
        )
        .expect("save settings");
        assert_eq!(saved["password_saved"].as_bool(), Some(true));
        store
            .put_app_setting("theme.accent", &json!("#38bdf8"))
            .expect("accent setting");

        let test = test_connection(&store, None).expect("test");
        assert_eq!(test["ok"].as_bool(), Some(true));

        let uploaded = upload_snapshot(
            &mut store,
            &json!({ "sync_password": "sync-pass", "device_name": "e2e-device" }),
        )
        .expect("upload");
        assert_eq!(uploaded["uploaded"].as_bool(), Some(true));
        let snapshot_id = uploaded["snapshot_id"].as_str().expect("sid").to_string();
        assert!(!snapshot_id.is_empty());

        let info = fetch_remote_info(&store).expect("info");
        assert_eq!(info["exists"].as_bool(), Some(true));
        assert_eq!(info["compatible"].as_bool(), Some(true));
        assert_eq!(info["snapshot_id"].as_str(), Some(snapshot_id.as_str()));
        assert_eq!(info["device_name"].as_str(), Some("e2e-device"));
        assert!(info["data_size"].as_u64().unwrap_or(0) > 0);

        // Fresh store downloads and restores secrets (settings must exist).
        let mut fresh = Store::open(&dir.join("fresh.db")).expect("fresh");
        let _ = settings_save(
            &mut fresh,
            &json!({
                "enabled": true,
                "base_url": base.clone(),
                "username": "webdav-user",
                "password": "webdav-pw",
                "password_touched": true,
                "remote_root": "mxterm-sync",
                "profile": "default",
            }),
        )
        .expect("fresh settings");
        let downloaded = download_snapshot(&mut fresh, &json!({ "sync_password": "sync-pass" }))
            .expect("download");
        assert_eq!(downloaded["downloaded"].as_bool(), Some(true));
        assert_eq!(
            downloaded["snapshot_id"].as_str(),
            Some(snapshot_id.as_str())
        );
        let restored = fresh
            .get_connection("conn-1")
            .expect("conn")
            .expect("exists");
        assert_eq!(restored["password"], "secret");
        let restored_cred = fresh
            .get_credential("cred-1")
            .expect("cred")
            .expect("exists");
        assert_eq!(restored_cred["password"], "pw-secret");
        let restored_setting = fresh
            .get_app_setting("theme.accent")
            .expect("setting")
            .expect("exists");
        assert_eq!(restored_setting, json!("#38bdf8"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(dir.join("fresh.db"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
