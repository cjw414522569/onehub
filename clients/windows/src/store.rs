//! Local SQLite persistence for the PC client (mxterm parity T001).
//!
//! Stores connection profiles, credentials, command snippets, and command
//! history in a local SQLite database so data survives restarts. Row shapes
//! mirror the mXterm frontend contracts (ConnectionProfile, CredentialProfile,
//! CommandSnippet, CommandHistoryEntry). This module is owned by the native
//! shell (clients/windows); the bridge stays pure and this store is applied
//! around bridge calls by `on_web_message` in main.rs.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

/// A connection profile row (subset of mXterm ConnectionProfile).
fn connection_json(
    id: &str,
    request: &Value,
    now: &str,
    existing: Option<&Value>,
    is_favorite: bool,
    last_connected_at: Option<&str>,
) -> Value {
    let name = request.get("name").and_then(Value::as_str).unwrap_or("");
    let host = request.get("host").and_then(Value::as_str).unwrap_or("");
    let username = request
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("");
    let port = request.get("port").and_then(Value::as_u64).unwrap_or(22);
    let protocol = request
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("ssh");
    let group = request.get("group").and_then(Value::as_str).unwrap_or("");
    let display = if name.is_empty() {
        host.to_string()
    } else {
        name.to_string()
    };
    json!({
        "id": id,
        "name": display,
        "protocol": protocol,
        "group": if group.is_empty() { Value::Null } else { json!(group) },
        "host": host,
        "port": port,
        "username": username,
        "credential_mode": request.get("credential_mode").and_then(Value::as_str).unwrap_or("prompt"),
        "credential_id": request.get("credential_id").cloned().unwrap_or(Value::Null),
        "proxy": request.get("proxy").cloned().unwrap_or(json!({ "kind": "none", "host": "", "port": 8080, "username": "", "password": "" })),
        "jump": request.get("jump").cloned().unwrap_or(json!({ "kind": "none", "jump_connection_id": "" })),
        "advanced": request.get("advanced").cloned().unwrap_or(json!({
            "auth_timeout_ms": 45000,
            "connect_timeout_ms": 30000,
            "keepalive_interval_ms": 20000,
            "terminal_encoding": "utf-8"
        })),
        "notes": request.get("notes").cloned().unwrap_or(Value::Null),
        "is_favorite": is_favorite,
        "last_connected_at": last_connected_at.map(|s| json!(s)).unwrap_or(Value::Null),
        "remote_os_id": request.get("remote_os_id").cloned().unwrap_or(Value::Null),
        "remote_os_name": request.get("remote_os_name").cloned().unwrap_or(Value::Null),
        "remote_os_version": request.get("remote_os_version").cloned().unwrap_or(Value::Null),
        "created_at": existing.and_then(|e| e.get("created_at").and_then(Value::as_str)).unwrap_or(now),
        "updated_at": now,
        "auth_kind": request.get("auth_kind").cloned().unwrap_or(Value::Null),
        "password": request.get("password").cloned().unwrap_or(Value::Null),
        "private_key_path": request.get("private_key_path").cloned().unwrap_or(Value::Null),
        "private_key_passphrase": request.get("private_key_passphrase").cloned().unwrap_or(Value::Null),
    })
}

/// A credential row (mXterm CredentialProfile).
fn credential_json(id: &str, request: &Value, now: &str, existing: Option<&Value>) -> Value {
    let name = request
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("credential");
    let kind = request
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("password");
    json!({
        "id": id,
        "name": name,
        "username": request.get("username").cloned().unwrap_or(Value::Null),
        "kind": kind,
        "password": request.get("password").cloned().unwrap_or(Value::Null),
        "private_key_path": request.get("private_key_path").cloned().unwrap_or(Value::Null),
        "private_key_passphrase": request.get("private_key_passphrase").cloned().unwrap_or(Value::Null),
        "notes": request.get("notes").cloned().unwrap_or(Value::Null),
        "created_at": existing.and_then(|e| e.get("created_at").and_then(Value::as_str)).unwrap_or(now),
        "updated_at": now,
    })
}

/// A command snippet row (mXterm CommandSnippet).
fn snippet_json(id: &str, request: &Value, now: &str, existing: Option<&Value>) -> Value {
    let command = request.get("command").and_then(Value::as_str).unwrap_or("");
    let use_count = existing
        .and_then(|e| e.get("use_count").and_then(Value::as_u64))
        .unwrap_or(0);
    json!({
        "id": id,
        "title": request.get("title").and_then(Value::as_str).unwrap_or(command),
        "command": command,
        "description": request.get("description").cloned().unwrap_or(Value::Null),
        "group": request.get("group").and_then(Value::as_str).unwrap_or(""),
        "tags": request.get("tags").cloned().unwrap_or(json!([])),
        "favorite": request.get("favorite").and_then(Value::as_bool).unwrap_or(false),
        "use_count": use_count,
        "last_used_at": existing.and_then(|e| e.get("last_used_at").cloned()).unwrap_or(Value::Null),
        "created_at": existing.and_then(|e| e.get("created_at").and_then(Value::as_str)).unwrap_or(now),
        "updated_at": now,
    })
}

/// A command history row (mXterm CommandHistoryEntry).
fn history_json(id: &str, request: &Value, now: &str) -> Value {
    json!({
        "id": id,
        "command": request.get("command").and_then(Value::as_str).unwrap_or(""),
        "source": request.get("source").and_then(Value::as_str).unwrap_or("terminal_input"),
        "target_count": request.get("target_count").and_then(Value::as_u64).unwrap_or(0),
        "append_enter": request.get("append_enter").and_then(Value::as_bool).unwrap_or(false),
        "use_count": 0,
        "last_used_at": now,
        "created_at": now,
    })
}

/// The local SQLite store.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the store at `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Opens an in-memory store (tests).
    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS items (
                 category TEXT NOT NULL,
                 id TEXT NOT NULL,
                 data TEXT NOT NULL,
                 PRIMARY KEY (category, id)
             );
             CREATE INDEX IF NOT EXISTS idx_items_category ON items(category);",
        )?;
        Ok(Self { conn })
    }

    fn get(&self, category: &str, id: &str) -> rusqlite::Result<Option<Value>> {
        let row = self
            .conn
            .query_row(
                "SELECT data FROM items WHERE category=?1 AND id=?2",
                params![category, id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(row.and_then(|s| serde_json::from_str(&s).ok()))
    }

    fn list(&self, category: &str) -> rusqlite::Result<Vec<Value>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM items WHERE category=?1 ORDER BY id")?;
        let rows = stmt
            .query_map(params![category], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|s| serde_json::from_str::<Value>(&s).ok())
            .collect();
        Ok(rows)
    }

    fn put(&mut self, category: &str, id: &str, data: &Value) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO items(category, id, data) VALUES(?1, ?2, ?3)
             ON CONFLICT(category, id) DO UPDATE SET data=excluded.data",
            params![category, id, data.to_string()],
        )?;
        Ok(())
    }

    fn delete(&mut self, category: &str, id: &str) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "DELETE FROM items WHERE category=?1 AND id=?2",
            params![category, id],
        )?;
        Ok(changed > 0)
    }

    /// A new unique id (prefix + counter + timestamp).
    fn new_id(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{prefix}-{nanos:x}")
    }

    fn now() -> String {
        // RFC3339-ish local timestamp (mXterm uses RFC3339 strings).
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{}", secs)
    }

    // ---- connections ----

    /// Lists all connection profiles (mXterm ConnectionProfile[]).
    pub fn list_connections(&self) -> rusqlite::Result<Vec<Value>> {
        self.list("connection")
    }

    /// Upserts a connection; returns the persisted profile.
    pub fn upsert_connection(&mut self, request: &Value) -> rusqlite::Result<Value> {
        let now = Self::now();
        let existing_id = request.get("id").and_then(Value::as_str);
        let existing = existing_id.and_then(|id| self.get("connection", id).ok().flatten());
        let id = existing_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| Self::new_id("conn"));
        let is_favorite = request
            .get("is_favorite")
            .and_then(Value::as_bool)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|e| e.get("is_favorite").and_then(Value::as_bool))
            })
            .unwrap_or(false);
        let last_connected_at = request
            .get("last_connected_at")
            .and_then(Value::as_str)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|e| e.get("last_connected_at").and_then(Value::as_str))
            });
        let profile = connection_json(
            &id,
            request,
            &now,
            existing.as_ref(),
            is_favorite,
            last_connected_at,
        );
        self.put("connection", &id, &profile)?;
        Ok(profile)
    }

    /// Deletes a connection; returns whether it existed.
    pub fn delete_connection(&mut self, id: &str) -> rusqlite::Result<bool> {
        self.delete("connection", id)
    }

    /// Marks a connection favorite.
    pub fn set_connection_favorite(
        &mut self,
        id: &str,
        favorite: bool,
    ) -> rusqlite::Result<Option<Value>> {
        let existing = self.get("connection", id)?;
        if let Some(mut profile) = existing {
            profile["is_favorite"] = json!(favorite);
            profile["updated_at"] = json!(Self::now());
            self.put("connection", id, &profile)?;
            Ok(Some(profile))
        } else {
            Ok(None)
        }
    }

    /// Marks a connection as connected.
    pub fn mark_connection_connected(&mut self, id: &str) -> rusqlite::Result<Option<Value>> {
        let existing = self.get("connection", id)?;
        if let Some(mut profile) = existing {
            profile["last_connected_at"] = json!(Self::now());
            profile["updated_at"] = json!(Self::now());
            self.put("connection", id, &profile)?;
            Ok(Some(profile))
        } else {
            Ok(None)
        }
    }

    // ---- credentials ----

    /// Lists credentials (mXterm CredentialProfile[]).
    pub fn list_credentials(&self) -> rusqlite::Result<Vec<Value>> {
        self.list("credential")
    }

    /// Upserts a credential; returns the persisted profile.
    pub fn upsert_credential(&mut self, request: &Value) -> rusqlite::Result<Value> {
        let now = Self::now();
        let existing_id = request.get("id").and_then(Value::as_str);
        let existing = existing_id.and_then(|id| self.get("credential", id).ok().flatten());
        let id = existing_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| Self::new_id("cred"));
        let cred = credential_json(&id, request, &now, existing.as_ref());
        self.put("credential", &id, &cred)?;
        Ok(cred)
    }

    /// Deletes a credential.
    pub fn delete_credential(&mut self, id: &str) -> rusqlite::Result<bool> {
        self.delete("credential", id)
    }

    // ---- command snippets ----

    /// Lists command snippets (mXterm CommandSnippet[]).
    pub fn list_command_snippets(&self) -> rusqlite::Result<Vec<Value>> {
        self.list("snippet")
    }

    /// Upserts a command snippet.
    pub fn upsert_command_snippet(&mut self, request: &Value) -> rusqlite::Result<Value> {
        let now = Self::now();
        let existing_id = request.get("id").and_then(Value::as_str);
        let existing = existing_id.and_then(|id| self.get("snippet", id).ok().flatten());
        let id = existing_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| Self::new_id("snip"));
        let snippet = snippet_json(&id, request, &now, existing.as_ref());
        self.put("snippet", &id, &snippet)?;
        Ok(snippet)
    }

    /// Deletes a command snippet.
    pub fn delete_command_snippet(&mut self, id: &str) -> rusqlite::Result<bool> {
        self.delete("snippet", id)
    }

    /// Marks a snippet used (increments use_count, sets last_used_at).
    pub fn mark_command_snippet_used(&mut self, id: &str) -> rusqlite::Result<Option<Value>> {
        let existing = self.get("snippet", id)?;
        if let Some(mut snippet) = existing {
            let count = snippet
                .get("use_count")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                + 1;
            snippet["use_count"] = json!(count);
            snippet["last_used_at"] = json!(Self::now());
            snippet["updated_at"] = json!(Self::now());
            self.put("snippet", id, &snippet)?;
            Ok(Some(snippet))
        } else {
            Ok(None)
        }
    }

    // ---- command history ----

    /// Lists command history (mXterm CommandHistoryEntry[]).
    pub fn list_command_history(&self) -> rusqlite::Result<Vec<Value>> {
        self.list("history")
    }

    /// Records a command history entry.
    pub fn record_command_history(&mut self, request: &Value) -> rusqlite::Result<Value> {
        let now = Self::now();
        let id = Self::new_id("hist");
        let entry = history_json(&id, request, &now);
        self.put("history", &id, &entry)?;
        Ok(entry)
    }

    /// Deletes a history entry.
    pub fn delete_command_history(&mut self, id: &str) -> rusqlite::Result<bool> {
        self.delete("history", id)
    }

    /// Clears all command history.
    pub fn clear_command_history(&mut self) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM items WHERE category=?1", params!["history"])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().expect("in-memory store")
    }

    #[test]
    fn connection_upsert_list_delete_roundtrip() {
        let mut s = store();
        let p = s
            .upsert_connection(
                &json!({ "name": "dev", "host": "10.0.0.1", "port": 22, "username": "root" }),
            )
            .expect("upsert");
        assert_eq!(p["name"], "dev");
        assert_eq!(p["host"], "10.0.0.1");
        let id = p["id"].as_str().expect("id").to_string();
        let list = s.list_connections().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["id"], id);
        assert!(s.delete_connection(&id).expect("delete"));
        assert_eq!(s.list_connections().expect("list").len(), 0);
    }

    #[test]
    fn connection_edit_preserves_created_at() {
        let mut s = store();
        let p = s
            .upsert_connection(
                &json!({ "id": "c1", "name": "a", "host": "h1", "port": 22, "username": "u" }),
            )
            .expect("upsert");
        let created = p["created_at"].as_str().expect("created").to_string();
        let p2 = s
            .upsert_connection(
                &json!({ "id": "c1", "name": "b", "host": "h2", "port": 23, "username": "v" }),
            )
            .expect("upsert2");
        assert_eq!(p2["created_at"], created);
        assert_eq!(p2["name"], "b");
    }

    #[test]
    fn credential_snippet_history_roundtrip() {
        let mut s = store();
        let c = s
            .upsert_credential(&json!({ "name": "prod", "kind": "password", "username": "root" }))
            .expect("cred");
        assert_eq!(c["kind"], "password");
        assert_eq!(s.list_credentials().expect("list").len(), 1);

        let sn = s
            .upsert_command_snippet(&json!({ "title": "ls", "command": "ls -la" }))
            .expect("snippet");
        let sn_id = sn["id"].as_str().expect("id").to_string();
        assert_eq!(s.list_command_snippets().expect("list").len(), 1);
        let used = s
            .mark_command_snippet_used(&sn_id)
            .expect("used")
            .expect("snippet exists");
        assert_eq!(used["use_count"], 1);

        let h = s
            .record_command_history(&json!({ "command": "ls", "source": "terminal_input" }))
            .expect("history");
        assert_eq!(h["command"], "ls");
        assert_eq!(s.list_command_history().expect("list").len(), 1);
        s.clear_command_history().expect("clear");
        assert_eq!(s.list_command_history().expect("list").len(), 0);
    }

    #[test]
    fn persistence_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("ssh-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db = dir.join("test.db");
        {
            let mut s = Store::open(&db).expect("open");
            s.upsert_connection(
                &json!({ "name": "persist", "host": "10.1.1.1", "port": 22, "username": "u" }),
            )
            .expect("upsert");
        }
        let s2 = Store::open(&db).expect("reopen");
        let list = s2.list_connections().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "persist");
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
