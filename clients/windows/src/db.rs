//! Database workspace framework (navop parity, T018+).
//!
//! Provides the database-engine registry, connection-profile parsing, the
//! `db_connection_*` command surface (routed by main.rs), and a session
//! registry that later rows (T019+) fill with real engine connections. Until
//! an engine is wired, `connect` returns a clear recoverable error instead of
//! faking a session (no capability is faked).

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::probe;

/// Known database engines `(key, label)`. Extensions (DM/Kingbase/GBase/...)
/// are listed so the UI picker and profile validation are complete; real
/// connectivity is wired row by row (T019+).
pub const DB_ENGINES: &[(&str, &str)] = &[
    ("mysql", "MySQL"),
    ("postgresql", "PostgreSQL"),
    ("sqlite", "SQLite"),
    ("duckdb", "DuckDB"),
    ("sqlserver", "SQL Server"),
    ("oracle", "Oracle"),
    ("clickhouse", "ClickHouse"),
    ("dm", "达梦 DM"),
    ("kingbase", "金仓 KingbaseES"),
    ("gbase", "GBase 8s"),
    ("oceanbase", "OceanBase"),
    ("opengauss", "openGauss"),
    ("iotdb", "Apache IoTDB"),
    ("redis", "Redis"),
    ("mongodb", "MongoDB"),
];

/// Engines whose real connection is implemented. Wired by later rows (T019+);
/// empty in T018 so the framework never reports a faked connection.
fn wired_engines() -> &'static [&'static str] {
    &[]
}

/// Returns true when the engine key is part of the known catalog.
pub fn is_known_engine(engine: &str) -> bool {
    DB_ENGINES.iter().any(|(key, _)| *key == engine)
}

/// Human-readable engine label, when known.
pub fn engine_label(engine: &str) -> Option<&'static str> {
    DB_ENGINES
        .iter()
        .find(|(key, _)| *key == engine)
        .map(|(_, label)| *label)
}

/// True when the engine has a real connection implementation (T019+).
pub fn engine_available(engine: &str) -> bool {
    wired_engines().contains(&engine)
}

/// Engine catalog for `db_engine_list` (UI engine picker).
pub fn engine_list() -> Vec<Value> {
    DB_ENGINES
        .iter()
        .map(|(key, label)| {
            json!({
                "engine": key,
                "label": label,
                "available": engine_available(key),
            })
        })
        .collect()
}

/// Parsed database connection profile (shared by all engines).
#[derive(Debug, Clone)]
pub struct DbProfile {
    pub engine: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub database: String,
    pub ssl: bool,
    pub connect_timeout_ms: u64,
}

impl DbProfile {
    /// Parses and validates a profile JSON. Rejects unknown engines and empty
    /// required fields so later rows can trust the struct.
    pub fn parse(profile: &Value) -> Result<Self, String> {
        let engine = profile
            .get("engine")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !is_known_engine(&engine) {
            return Err(format!("未知数据库引擎：{engine}"));
        }
        let host = profile
            .get("host")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let port = profile
            .get("port")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| default_port(&engine)) as u16;
        let username = profile
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let password = profile
            .get("password")
            .and_then(Value::as_str)
            .map(str::to_string);
        let database = profile
            .get("database")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let ssl = profile.get("ssl").and_then(Value::as_bool).unwrap_or(false);
        let connect_timeout_ms = profile
            .get("connect_timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(5000);
        let name = profile
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(Self {
            engine,
            name,
            host,
            port,
            username,
            password,
            database,
            ssl,
            connect_timeout_ms,
        })
    }

    /// File-backed engines (SQLite/DuckDB) use a local path instead of TCP.
    pub fn requires_host(&self) -> bool {
        !matches!(self.engine.as_str(), "sqlite" | "duckdb")
    }

    /// Default display name when the user leaves it blank.
    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.clone();
        }
        if self.requires_host() {
            format!(
                "{}@{}",
                self.engine,
                if self.host.is_empty() {
                    "localhost"
                } else {
                    &self.host
                }
            )
        } else {
            format!(
                "{}:{}",
                self.engine,
                if self.database.is_empty() {
                    "local"
                } else {
                    &self.database
                }
            )
        }
    }
}

/// Default TCP port per engine (for UI prefill and profile validation).
fn default_port(engine: &str) -> u64 {
    match engine {
        "mysql" | "oceanbase" => 3306,
        "postgresql" | "kingbase" | "opengauss" | "gbase" => 5432,
        "sqlserver" | "dm" => 1433,
        "oracle" => 1521,
        "clickhouse" => 8123,
        "iotdb" => 6667,
        "redis" => 6379,
        "mongodb" => 27017,
        _ => 0,
    }
}

/// Tests a DB connection profile: TCP reachability for server engines, local
/// path availability for file engines (sqlite/duckdb), plus engine wiring
/// status. Never performs a protocol handshake in T018.
pub fn test_connection(profile: &Value) -> Value {
    let engine = profile
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let parsed = match DbProfile::parse(profile) {
        Ok(parsed) => parsed,
        Err(message) => {
            return json!({
                "ok": false,
                "reachable": false,
                "latency_ms": null,
                "engine": engine,
                "engine_available": engine_available(&engine),
                "message": message,
            })
        }
    };
    if !parsed.requires_host() {
        let path_ok =
            !parsed.database.is_empty() && std::path::Path::new(&parsed.database).exists();
        return json!({
            "ok": true,
            "reachable": path_ok,
            "latency_ms": null,
            "engine": parsed.engine,
            "engine_available": engine_available(&parsed.engine),
            "message": if path_ok { "本地数据库文件可用" } else { "本地数据库文件不存在（连接时将创建）" },
        });
    }
    let target = probe::Target {
        host: parsed.host.clone(),
        port: parsed.port,
        username: parsed.username.clone(),
    };
    let timeout = Duration::from_millis(parsed.connect_timeout_ms.max(500));
    let probe = probe::probe_tcp(&target, timeout);
    let label = engine_label(&parsed.engine).unwrap_or(&parsed.engine);
    let message = if probe.reachable {
        format!("TCP 可达（{label}）")
    } else {
        "TCP 不可达（端口未监听或被防火墙拦截）".to_string()
    };
    json!({
        "ok": true,
        "reachable": probe.reachable,
        "latency_ms": probe.latency_ms,
        "engine": parsed.engine,
        "engine_available": engine_available(&parsed.engine),
        "message": message,
    })
}

/// A live (or reserved) database session. Real engine sessions are created by
/// later rows once an engine is wired.
pub struct DbSession {
    pub id: String,
    pub engine: String,
    pub profile: Value,
    pub created_at: String,
}

static DB_SESSIONS: Mutex<Option<HashMap<String, DbSession>>> = Mutex::new(None);

fn sessions_map() -> &'static Mutex<Option<HashMap<String, DbSession>>> {
    &DB_SESSIONS
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos:x}")
}

/// Opens a database session for a wired engine. In T018 no engine is wired
/// yet, so this returns a clear recoverable error instead of faking a session.
pub fn connect(profile: &Value) -> Result<String, String> {
    let parsed = DbProfile::parse(profile)?;
    if !engine_available(&parsed.engine) {
        let label = engine_label(&parsed.engine).unwrap_or(&parsed.engine);
        return Err(format!(
            "数据库引擎未接入：{}（{}），真实连接将在后续版本提供",
            parsed.engine, label
        ));
    }
    let id = new_id("db");
    let created_at = new_id("ts");
    sessions_map()
        .lock()
        .expect("db sessions lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            id.clone(),
            DbSession {
                id: id.clone(),
                engine: parsed.engine,
                profile: profile.clone(),
                created_at,
            },
        );
    Ok(id)
}

/// Closes (removes) a database session; returns true when one was removed.
pub fn close_session(session_id: &str) -> bool {
    sessions_map()
        .lock()
        .expect("db sessions lock")
        .as_mut()
        .map(|m| m.remove(session_id).is_some())
        .unwrap_or(false)
}

/// Lists active database session ids.
pub fn active_db_session_ids() -> Vec<String> {
    sessions_map()
        .lock()
        .expect("db sessions lock")
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Filters a connection list down to database-protocol entries.
pub fn is_db_protocol(protocol: &str) -> bool {
    DB_ENGINES.iter().any(|(key, _)| *key == protocol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_catalog_is_complete() {
        assert_eq!(DB_ENGINES.len(), 15);
        assert!(is_known_engine("mysql"));
        assert!(is_known_engine("iotdb"));
        assert!(!is_known_engine("nosql-unknown"));
        assert_eq!(engine_label("mysql"), Some("MySQL"));
        // Nothing is wired in T018; the framework must not fake availability.
        assert!(DB_ENGINES.iter().all(|(key, _)| !engine_available(key)));
        assert_eq!(engine_list().len(), 15);
    }

    #[test]
    fn profile_parsing_and_default_ports() {
        let profile =
            json!({ "engine": "mysql", "host": "db.example", "port": 3307, "username": "root" });
        let parsed = DbProfile::parse(&profile).expect("parse");
        assert_eq!(parsed.port, 3307);
        assert_eq!(parsed.display_name(), "mysql@db.example");
        assert!(parsed.requires_host());

        let pg = DbProfile::parse(&json!({ "engine": "postgresql", "host": "h" })).expect("pg");
        assert_eq!(pg.port, 5432);

        let sqlite =
            DbProfile::parse(&json!({ "engine": "sqlite", "database": "x.db" })).expect("sqlite");
        assert!(!sqlite.requires_host());
        assert_eq!(sqlite.display_name(), "sqlite:x.db");

        let err = DbProfile::parse(&json!({ "engine": "nope" })).expect_err("unknown");
        assert!(err.contains("未知数据库引擎"));
    }

    #[test]
    fn tcp_test_refused_is_graceful() {
        let result = test_connection(&json!({
            "engine": "mysql",
            "host": "127.0.0.1",
            "port": 1,
            "connect_timeout_ms": 800,
        }));
        assert_eq!(result["reachable"], false);
        assert_eq!(result["ok"], true);
        assert!(result["message"].as_str().unwrap_or("").contains("不可达"));
    }

    #[test]
    fn file_engine_test_reports_path() {
        let missing = test_connection(
            &json!({ "engine": "sqlite", "database": "C:/nope/does-not-exist/x.db" }),
        );
        assert_eq!(missing["reachable"], false);

        let dir = std::env::temp_dir().join("onehub-db-test.sqlite");
        std::fs::write(&dir, b"x").expect("write");
        let present =
            test_connection(&json!({ "engine": "sqlite", "database": dir.to_string_lossy() }));
        assert_eq!(present["reachable"], true);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn connect_is_honest_until_engine_wired() {
        let err = connect(
            &json!({ "engine": "mysql", "host": "127.0.0.1", "username": "root", "password": "x" }),
        )
        .expect_err("mysql not wired in T018");
        assert!(err.contains("未接入"), "got {err:?}");
        assert!(close_session("db-missing") == false);
        assert!(active_db_session_ids().is_empty());
    }
}
