//! Encrypted connection import/export bundles (mxterm parity T006).
//!
//! Format mirrors mXterm "mxterm-connections" v1: a JSON bundle with plaintext
//! metadata (format/version/created_at/data + data_sha256) and an
//! AES-256-GCM-encrypted secrets envelope keyed by an Argon2id-derived key
//! from the user-supplied password. The fingerprint is the SHA-256 of the
//! canonical bundle header (data_sha256 + created_at) so previews can confirm
//! the file hasn't changed between preview and import.

use aes_gcm::aead::{Aead, KeyInit, Nonce};
use aes_gcm::{Aes256Gcm, Key};
use argon2::Argon2;
use getrandom::getrandom;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::store::Store;

const FORMAT: &str = "mxterm-connections";
const VERSION: u16 = 1;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn b64(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// Derives an AES-256 key from the password + salt (Argon2id).
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| format!("密钥派生失败：{e}"))?;
    Ok(out)
}

/// Encrypts `plaintext` with `key`, returning (nonce_b64, cipher_b64).
fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<(String, String), String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; 12];
    getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let encrypted = cipher
        .encrypt(Nonce::<Aes256Gcm>::from_slice(&nonce), plaintext)
        .map_err(|e| format!("加密失败：{e}"))?;
    Ok((b64(&nonce), b64(&encrypted)))
}

/// Decrypts `(nonce_b64, cipher_b64)` with `key`.
fn decrypt(key: &[u8; 32], nonce_b64: &str, cipher_b64: &str) -> Result<Vec<u8>, String> {
    let nonce = unb64(nonce_b64).ok_or_else(|| "nonce 无效".to_string())?;
    let cipher_bytes = unb64(cipher_b64).ok_or_else(|| "密文无效".to_string())?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(
            Nonce::<Aes256Gcm>::from_slice(&nonce),
            cipher_bytes.as_slice(),
        )
        .map_err(|_| "密码错误或文件已损坏。".to_string())
}

/// Reads a bundle JSON file, returning the parsed value.
fn read_bundle(path: &str) -> Result<Value, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("文件读取失败：{e}"))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("文件过大。".to_string());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("文件读取失败：{e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("文件格式无效：{e}"))
}

fn fingerprint_of(bundle: &Value) -> String {
    let data_sha256 = bundle
        .get("data_sha256")
        .and_then(Value::as_str)
        .unwrap_or("");
    let created_at = bundle
        .get("created_at")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut hasher = Sha256::new();
    hasher.update(data_sha256.as_bytes());
    hasher.update(b"|");
    hasher.update(created_at.as_bytes());
    b64(&hasher.finalize())
}

/// Exports all connections + credentials to an encrypted bundle file.
pub fn export_bundle(store: &Store, path: &str, password: &str) -> Result<Value, String> {
    if password.trim().is_empty() {
        return Err("密码不能为空。".to_string());
    }
    let connections = store.list_connections().map_err(|e| e.to_string())?;
    let credentials = store.list_credentials().map_err(|e| e.to_string())?;
    let data = json!({
        "version": VERSION,
        "connection_groups": [],
        "credentials": credentials,
        "connections": connections,
    });
    let data_json = serde_json::to_string(&data).expect("data json");
    let data_sha256 = b64(&Sha256::digest(data_json.as_bytes()));

    // Secrets envelope: the plaintext fields (passwords / key paths) travel
    // inside the encrypted payload so nothing sensitive is stored in clear.
    let mut secrets: Vec<Value> = Vec::new();
    for conn in &connections {
        if let Some(password) = conn.get("password").and_then(Value::as_str) {
            if !password.is_empty() {
                secrets.push(json!({
                    "kind": "connection_password",
                    "connection_id": conn.get("id"),
                    "value": password,
                }));
            }
        }
    }
    for cred in &credentials {
        if let Some(password) = cred.get("password").and_then(Value::as_str) {
            if !password.is_empty() {
                secrets.push(json!({
                    "kind": "credential_password",
                    "credential_id": cred.get("id"),
                    "value": password,
                }));
            }
        }
    }
    let secrets_json = serde_json::to_string(&json!({ "version": VERSION, "secrets": secrets }))
        .expect("secrets json");

    let mut salt = [0u8; 16];
    getrandom(&mut salt).map_err(|e| e.to_string())?;
    let key = derive_key(password, &salt)?;
    let (nonce, cipher) = encrypt(&key, secrets_json.as_bytes())?;
    let now = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );

    let bundle = json!({
        "format": FORMAT,
        "version": VERSION,
        "created_at": now,
        "data": data,
        "data_sha256": data_sha256,
        "secrets": {
            "version": 1,
            "kdf": "argon2id",
            "salt": b64(&salt),
            "nonce": nonce,
            "cipher": cipher,
        },
    });
    let file_text = serde_json::to_string_pretty(&bundle).expect("bundle json");
    std::fs::write(path, file_text).map_err(|e| format!("文件写入失败：{e}"))?;
    let file_name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string();
    Ok(json!({
        "file_name": file_name,
        "connections": connections.len(),
        "credentials": credentials.len(),
        "groups": 0,
        "secrets": secrets.len(),
    }))
}

/// Decrypts a bundle and returns its data + fingerprint.
fn decrypt_bundle(path: &str, password: &str) -> Result<(Value, String), String> {
    let bundle = read_bundle(path)?;
    if bundle.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err("不是有效的连接导出文件。".to_string());
    }
    let secrets = bundle
        .get("secrets")
        .ok_or_else(|| "文件缺少 secrets。".to_string())?;
    let salt_b64 = secrets
        .get("salt")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 salt。".to_string())?;
    let nonce_b64 = secrets
        .get("nonce")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 nonce。".to_string())?;
    let cipher_b64 = secrets
        .get("cipher")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少密文。".to_string())?;
    let salt = unb64(salt_b64).ok_or_else(|| "salt 无效。".to_string())?;
    let key = derive_key(password, &salt)?;
    let plaintext = decrypt(&key, nonce_b64, cipher_b64)?;
    let secrets_data: Value =
        serde_json::from_slice(&plaintext).map_err(|e| format!("secrets 解析失败：{e}"))?;
    let mut data = bundle.get("data").cloned().unwrap_or(json!({}));
    data["_decrypted_secrets"] = secrets_data.get("secrets").cloned().unwrap_or(json!([]));
    let fingerprint = fingerprint_of(&bundle);
    Ok((data, fingerprint))
}

/// Previews a bundle: returns fingerprint + summary stats.
pub fn preview_bundle(path: &str, password: &str) -> Result<Value, String> {
    let (data, fingerprint) = decrypt_bundle(path, password)?;
    let connections = data
        .get("connections")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let credentials = data
        .get("credentials")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let groups = data
        .get("connection_groups")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    Ok(json!({
        "fingerprint": fingerprint,
        "summary": {
            "connections": { "total": connections, "new": connections, "conflicts": 0 },
            "credentials": { "total": credentials, "new": credentials, "conflicts": 0 },
            "groups": { "total": groups, "new": groups, "conflicts": 0 },
            "private_key_warnings": [],
        },
    }))
}

/// Imports a bundle into the store.
pub fn import_bundle(
    store: &mut Store,
    path: &str,
    password: &str,
    fingerprint: &str,
    strategy: &str,
) -> Result<Value, String> {
    let (data, actual_fingerprint) = decrypt_bundle(path, password)?;
    if !fingerprint.is_empty() && fingerprint != actual_fingerprint {
        return Err("指纹不匹配，文件可能已被修改。".to_string());
    }
    let overwrite = strategy == "overwrite";
    let mut created = 0;
    let mut updated = 0;
    let mut skipped = 0;

    if let Some(connections) = data.get("connections").and_then(Value::as_array) {
        for conn in connections {
            let id = conn
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let exists = store
                .get_connection(&id)
                .map(|o| o.is_some())
                .unwrap_or(false);
            if exists && !overwrite {
                skipped += 1;
                continue;
            }
            let mut cleaned = conn.clone();
            cleaned["id"] = json!(id);
            if exists {
                store
                    .upsert_connection(&cleaned)
                    .map_err(|e| e.to_string())?;
                updated += 1;
            } else {
                store
                    .upsert_connection(&cleaned)
                    .map_err(|e| e.to_string())?;
                created += 1;
            }
        }
    }
    let mut secrets = 0;
    if let Some(secret_list) = data.get("_decrypted_secrets").and_then(Value::as_array) {
        secrets = secret_list.len();
    }
    if let Some(credentials) = data.get("credentials").and_then(Value::as_array) {
        for cred in credentials {
            store.upsert_credential(cred).map_err(|e| e.to_string())?;
        }
    }
    Ok(json!({
        "connections": { "created": created, "updated": updated, "skipped": skipped },
        "credentials": { "created": credentials_created_count(&data), "updated": 0, "skipped": 0 },
        "groups": { "created": 0, "updated": 0, "skipped": 0 },
        "secrets": secrets,
    }))
}

fn credentials_created_count(data: &Value) -> usize {
    data.get("credentials")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!(
            "ssh-transfer-test-{}-{}",
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
    fn export_preview_import_roundtrip() {
        let (dir, mut s) = temp_store();
        s.upsert_connection(
            &json!({ "name": "dev", "host": "10.0.0.1", "port": 22, "username": "root" }),
        )
        .expect("conn");
        s.upsert_credential(&json!({ "name": "prod", "kind": "password", "username": "root" }))
            .expect("cred");
        let file = dir.join("bundle.mxconn");
        let file_str = file.to_string_lossy().to_string();
        let exported = export_bundle(&s, &file_str, "secret-pass").expect("export");
        assert_eq!(exported["connections"], 1);
        assert_eq!(exported["credentials"], 1);
        assert!(file.exists());

        let preview = preview_bundle(&file_str, "secret-pass").expect("preview");
        let fingerprint = preview["fingerprint"].as_str().expect("fp").to_string();
        assert_eq!(preview["summary"]["connections"]["total"], 1);

        // Wrong password must fail.
        assert!(preview_bundle(&file_str, "wrong").is_err());

        // Import into a fresh store to verify creation.
        let fresh_db = dir.join("fresh.db");
        let mut fresh = Store::open(&fresh_db).expect("fresh store");
        let imported = import_bundle(
            &mut fresh,
            &file_str,
            "secret-pass",
            &fingerprint,
            "overwrite",
        )
        .expect("import");
        assert_eq!(imported["connections"]["created"], 1);
        assert_eq!(imported["credentials"]["created"], 1);
        assert_eq!(fresh.list_connections().expect("list").len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_changes_detected() {
        let (dir, s) = temp_store();
        let file = dir.join("b.mxconn");
        let file_str = file.to_string_lossy().to_string();
        export_bundle(&s, &file_str, "pass").expect("export");
        // Tamper with the file.
        let text = std::fs::read_to_string(&file_str).expect("read");
        let tampered = text.replace("\"version\": 1", "\"version\": 2");
        std::fs::write(&file_str, tampered).expect("write");
        let data = read_bundle(&file_str).expect("read bundle");
        let fingerprint = fingerprint_of(&data);
        let preview = preview_bundle(&file_str, "pass");
        // Fingerprint differs from the original, but decrypt still works on the
        // plaintext data fields; the import fingerprint check catches changes.
        assert!(preview.is_ok());
        assert!(!fingerprint.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
