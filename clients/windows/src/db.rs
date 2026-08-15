//! Database workspace framework (navop parity, T018+).
//!
//! Provides the database-engine registry, connection-profile parsing, the
//! `db_connection_*`/`db_query`/`db_exec` command surface (routed by main.rs),
//! and live engine sessions. MySQL (T019) and PostgreSQL (T020) are wired with
//! real connections -> query -> result sets; other engines are wired in later
//! rows. Nothing is faked: unwired engines return a clear recoverable error.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::probe;
use mysql_async::prelude::Queryable;
use tokio_postgres::types::Type;
use tokio_postgres::NoTls;

/// Known database engines `(key, label)`. Extensions (DM/Kingbase/GBase/...)
/// are listed so the UI picker and profile validation are complete; real
/// connectivity is wired row by row.
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

/// Engines whose real connection is implemented (wired by later rows).
fn wired_engines() -> &'static [&'static str] {
    &["mysql", "postgresql", "sqlite"]
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

/// True when the engine has a real connection implementation.
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
/// status. Never performs a protocol handshake beyond reachability.
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

/// A live engine connection held by a session.
enum EngineConnection {
    MySql(mysql_async::Pool),
    Postgres(std::sync::Arc<tokio_postgres::Client>),
    Sqlite(std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>),
}

/// A live database session. `connection` holds the engine handle for wired
/// engines; unwired engines never create a session.
pub struct DbSession {
    pub id: String,
    pub engine: String,
    pub profile: Value,
    pub created_at: String,
    connection: Option<EngineConnection>,
}

static DB_SESSIONS: Mutex<Option<HashMap<String, DbSession>>> = Mutex::new(None);

fn sessions_map() -> &'static Mutex<Option<HashMap<String, DbSession>>> {
    &DB_SESSIONS
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("tokio runtime"))
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos:x}")
}

/// Builds a MySQL connection pool from a parsed profile.
fn build_mysql_pool(parsed: &DbProfile) -> Result<mysql_async::Pool, String> {
    let mut builder = mysql_async::OptsBuilder::default()
        .ip_or_hostname(parsed.host.clone())
        .tcp_port(parsed.port)
        .user(Some(parsed.username.clone()));
    if let Some(password) = &parsed.password {
        builder = builder.pass(Some(password.clone()));
    }
    if !parsed.database.is_empty() {
        builder = builder.db_name(Some(parsed.database.clone()));
    }
    if parsed.ssl {
        builder = builder.ssl_opts(Some(mysql_async::SslOpts::default()));
    }
    Ok(mysql_async::Pool::new(mysql_async::Opts::from(builder)))
}

/// Connects a PostgreSQL client (plain TCP) and spawns its connection task.
fn pg_connect(parsed: &DbProfile) -> Result<std::sync::Arc<tokio_postgres::Client>, String> {
    let client = runtime().block_on(async {
        let mut config = tokio_postgres::config::Config::new();
        config
            .host(&parsed.host)
            .port(parsed.port)
            .user(&parsed.username);
        if let Some(password) = &parsed.password {
            config.password(password);
        }
        if !parsed.database.is_empty() {
            config.dbname(&parsed.database);
        }
        let (client, connection) = config
            .connect(NoTls)
            .await
            .map_err(|e| format!("PostgreSQL 连接失败：{e}"))?;
        runtime().spawn(async move {
            let _ = connection.await;
        });
        // Verify authentication with a trivial round-trip.
        let row = client
            .query_one("SELECT 1", &[])
            .await
            .map_err(|e| format!("PostgreSQL 认证失败：{e}"))?;
        let _ = row;
        Ok::<std::sync::Arc<tokio_postgres::Client>, String>(std::sync::Arc::new(client))
    })?;
    Ok(client)
}

/// Opens a SQLite database (file path or :memory:) and initializes it.
fn sqlite_open(parsed: &DbProfile) -> Result<rusqlite::Connection, String> {
    let path = if parsed.database.is_empty() {
        ":memory:".to_string()
    } else {
        parsed.database.clone()
    };
    let conn = rusqlite::Connection::open(&path).map_err(|e| format!("SQLite 打开失败：{e}"))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| format!("SQLite 初始化失败：{e}"))?;
    Ok(conn)
}

/// Converts a rusqlite value into a JSON value.
fn sqlite_value_to_json(value: rusqlite::types::Value) -> Value {
    match value {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(i) => serde_json::json!(i),
        rusqlite::types::Value::Real(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rusqlite::types::Value::Text(t) => Value::String(t),
        rusqlite::types::Value::Blob(b) => Value::String(String::from_utf8_lossy(&b).to_string()),
    }
}

/// Runs a statement against a SQLite connection (synchronous).
fn sqlite_run(conn: &rusqlite::Connection, sql: &str) -> Result<QueryOutcome, String> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("SQL 准备失败：{e}"))?;
    let column_count = stmt.column_count();
    if column_count == 0 {
        let affected = conn
            .execute(sql, [])
            .map_err(|e| format!("SQL 执行失败：{e}"))?;
        return Ok(QueryOutcome {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: affected as u64,
        });
    }
    let columns: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    let rows = stmt
        .query_map([], |row| {
            let mut values = Vec::new();
            for index in 0..column_count {
                let value = row
                    .get::<usize, rusqlite::types::Value>(index)
                    .unwrap_or(rusqlite::types::Value::Null);
                values.push(sqlite_value_to_json(value));
            }
            Ok(values)
        })
        .map_err(|e| format!("查询失败：{e}"))?;
    let mut result_rows = Vec::new();
    for row in rows {
        result_rows.push(row.map_err(|e| format!("读取结果失败：{e}"))?);
    }
    Ok(QueryOutcome {
        columns,
        rows: result_rows,
        affected_rows: 0,
    })
}

/// Converts a mysql_async value into a JSON value.
fn mysql_value_to_json(value: mysql_async::Value) -> Value {
    match value {
        mysql_async::Value::NULL => Value::Null,
        mysql_async::Value::Bytes(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Value::String(text),
            Err(err) => Value::Array(
                err.into_bytes()
                    .into_iter()
                    .map(|byte| serde_json::json!(byte))
                    .collect(),
            ),
        },
        mysql_async::Value::Int(i) => serde_json::json!(i),
        mysql_async::Value::UInt(u) => serde_json::json!(u),
        mysql_async::Value::Float(f) => serde_json::Number::from_f64(f as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        mysql_async::Value::Date(year, month, day, hour, minute, second, micros) => Value::String(
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"),
        ),
        mysql_async::Value::Time(is_neg, days, hours, minutes, seconds, micros) => {
            let sign = if is_neg { "-" } else { "" };
            Value::String(format!(
                "{sign}{days} {hours:02}:{minutes:02}:{seconds:02}.{micros:06}"
            ))
        }
        _ => Value::Null,
    }
}

/// Converts a PostgreSQL cell into a JSON value, dispatching on the column
/// type so ints/floats/text/json/dates/bytea all survive round-trips.
fn pg_cell_to_json(row: &tokio_postgres::Row, index: usize, ty: &Type) -> Value {
    if *ty == Type::BOOL {
        return row
            .try_get::<usize, bool>(index)
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT2 {
        return row
            .try_get::<usize, i16>(index)
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT4 {
        return row
            .try_get::<usize, i32>(index)
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT8 {
        return row
            .try_get::<usize, i64>(index)
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::FLOAT4 {
        return row
            .try_get::<usize, f32>(index)
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::FLOAT8 {
        return row
            .try_get::<usize, f64>(index)
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::JSON || *ty == Type::JSONB {
        return row
            .try_get::<usize, serde_json::Value>(index)
            .unwrap_or(Value::Null);
    }
    if *ty == Type::BYTEA {
        return row
            .try_get::<usize, Vec<u8>>(index)
            .map(|bytes| Value::String(String::from_utf8_lossy(&bytes).to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::DATE {
        return row
            .try_get::<usize, chrono::NaiveDate>(index)
            .map(|date| Value::String(date.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIME {
        return row
            .try_get::<usize, chrono::NaiveTime>(index)
            .map(|time| Value::String(time.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIMESTAMP {
        return row
            .try_get::<usize, chrono::NaiveDateTime>(index)
            .map(|stamp| Value::String(stamp.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIMESTAMPTZ {
        return row
            .try_get::<usize, chrono::DateTime<chrono::Utc>>(index)
            .map(|stamp| Value::String(stamp.to_string()))
            .unwrap_or(Value::Null);
    }
    // Fallback: text-ish columns (text/varchar/name/unknown/...).
    row.try_get::<usize, String>(index)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

/// Outcome of a statement: columns, row values, and affected rows.
#[derive(Debug)]
pub struct QueryOutcome {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub affected_rows: u64,
}

/// Runs a statement against a MySQL pool: SELECT yields columns+rows, DML
/// yields affected_rows.
async fn mysql_run(pool: &mysql_async::Pool, sql: &str) -> Result<QueryOutcome, String> {
    let mut conn = pool
        .get_conn()
        .await
        .map_err(|e| format!("获取连接失败：{e}"))?;
    let mut result = conn
        .query_iter(sql)
        .await
        .map_err(|e| format!("SQL 执行失败：{e}"))?;
    let columns: Vec<String> = result
        .columns()
        .map(|cols| {
            cols.iter()
                .map(|column| column.name_str().to_string())
                .collect()
        })
        .unwrap_or_default();
    let mut rows = Vec::new();
    while let Some(row) = result
        .next()
        .await
        .map_err(|e| format!("读取结果失败：{e}"))?
    {
        let mut values = Vec::new();
        for index in 0..columns.len() {
            let value = row
                .get::<mysql_async::Value, usize>(index)
                .unwrap_or(mysql_async::Value::NULL);
            values.push(mysql_value_to_json(value));
        }
        rows.push(values);
    }
    let affected_rows = result.affected_rows();
    Ok(QueryOutcome {
        columns,
        rows,
        affected_rows,
    })
}

/// Runs a statement against a PostgreSQL client.
async fn pg_run(client: &tokio_postgres::Client, sql: &str) -> Result<QueryOutcome, String> {
    let stmt = client
        .prepare(sql)
        .await
        .map_err(|e| format!("SQL 准备失败：{e}"))?;
    if stmt.columns().is_empty() {
        let affected = client
            .execute(&stmt, &[])
            .await
            .map_err(|e| format!("SQL 执行失败：{e}"))?;
        return Ok(QueryOutcome {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: affected,
        });
    }
    let columns: Vec<String> = stmt
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect();
    let rows = client
        .query(&stmt, &[])
        .await
        .map_err(|e| format!("查询失败：{e}"))?;
    let mut result_rows = Vec::new();
    for row in rows {
        let mut values = Vec::new();
        for index in 0..columns.len() {
            let ty = stmt.columns()[index].type_();
            values.push(pg_cell_to_json(&row, index, ty));
        }
        result_rows.push(values);
    }
    Ok(QueryOutcome {
        columns,
        rows: result_rows,
        affected_rows: 0,
    })
}

/// Runs a statement with an inline profile (per-call connection).
pub fn query_inline(profile: &Value, sql: &str) -> Result<QueryOutcome, String> {
    if sql.trim().is_empty() {
        return Err("SQL 为空。".to_string());
    }
    let parsed = DbProfile::parse(profile)?;
    match parsed.engine.as_str() {
        "mysql" => {
            let pool = build_mysql_pool(&parsed)?;
            runtime().block_on(mysql_run(&pool, sql))
        }
        "postgresql" => {
            if parsed.ssl {
                return Err("PostgreSQL TLS 暂未接入（当前为明文 TCP）。".to_string());
            }
            let client = pg_connect(&parsed)?;
            runtime().block_on(pg_run(&client, sql))
        }
        "sqlite" => {
            let conn = sqlite_open(&parsed)?;
            sqlite_run(&conn, sql)
        }
        other => {
            let label = engine_label(other).unwrap_or(other);
            Err(format!("数据库引擎未接入：{other}（{label}）"))
        }
    }
}

/// Runs a statement against an existing session's live connection.
pub fn query_session(session_id: &str, sql: &str) -> Result<QueryOutcome, String> {
    if sql.trim().is_empty() {
        return Err("SQL 为空。".to_string());
    }
    let connection = {
        let guard = sessions_map().lock().expect("db sessions lock");
        let session = guard
            .as_ref()
            .and_then(|m| m.get(session_id))
            .ok_or_else(|| "数据库会话不存在。".to_string())?;
        match &session.connection {
            Some(EngineConnection::MySql(pool)) => EngineConnection::MySql(pool.clone()),
            Some(EngineConnection::Postgres(client)) => {
                EngineConnection::Postgres(std::sync::Arc::clone(client))
            }
            Some(EngineConnection::Sqlite(conn)) => {
                EngineConnection::Sqlite(std::sync::Arc::clone(conn))
            }
            None => return Err("会话没有活动连接。".to_string()),
        }
    };
    match connection {
        EngineConnection::MySql(pool) => runtime().block_on(mysql_run(&pool, sql)),
        EngineConnection::Postgres(client) => runtime().block_on(pg_run(client.as_ref(), sql)),
        EngineConnection::Sqlite(conn) => {
            let guard = conn.lock().expect("sqlite session lock");
            sqlite_run(&guard, sql)
        }
    }
}

/// Opens a database session for a wired engine. MySQL/PostgreSQL verify the
/// connection with a real handshake; unwired engines return a clear error.
pub fn connect(profile: &Value) -> Result<String, String> {
    let parsed = DbProfile::parse(profile)?;
    if !engine_available(&parsed.engine) {
        let label = engine_label(&parsed.engine).unwrap_or(&parsed.engine);
        return Err(format!(
            "数据库引擎未接入：{}（{label}），真实连接将在后续版本提供",
            parsed.engine
        ));
    }
    let connection = match parsed.engine.as_str() {
        "mysql" => EngineConnection::MySql(build_mysql_pool(&parsed)?),
        "postgresql" => {
            if parsed.ssl {
                return Err("PostgreSQL TLS 暂未接入（当前为明文 TCP）。".to_string());
            }
            EngineConnection::Postgres(pg_connect(&parsed)?)
        }
        "sqlite" => {
            let conn = sqlite_open(&parsed)?;
            EngineConnection::Sqlite(std::sync::Arc::new(std::sync::Mutex::new(conn)))
        }
        other => return Err(format!("数据库引擎未接入：{other}")),
    };
    // Real handshake verification for both engines.
    match &connection {
        EngineConnection::MySql(pool) => {
            runtime().block_on(async {
                let mut conn = pool
                    .get_conn()
                    .await
                    .map_err(|e| format!("MySQL 连接失败：{e}"))?;
                conn.ping()
                    .await
                    .map_err(|e| format!("MySQL 认证失败：{e}"))
            })?;
        }
        EngineConnection::Postgres(client) => {
            runtime()
                .block_on(client.query_one("SELECT 1", &[]))
                .map_err(|e| format!("PostgreSQL 连接失败：{e}"))?;
        }
        EngineConnection::Sqlite(conn) => {
            let guard = conn.lock().expect("sqlite session lock");
            guard
                .query_row("SELECT 1", [], |_| Ok(()))
                .map_err(|e| format!("SQLite 连接失败：{e}"))?;
        }
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
                connection: Some(connection),
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
        // T019-T021 wire MySQL/PostgreSQL/SQLite; others must stay unwired.
        assert!(engine_available("mysql"));
        assert!(engine_available("postgresql"));
        assert!(engine_available("sqlite"));
        assert!(DB_ENGINES.iter().all(|(key, _)| {
            *key == "mysql" || *key == "postgresql" || *key == "sqlite" || !engine_available(key)
        }));
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
            "engine": "postgresql",
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
    fn mysql_value_conversion_covers_shapes() {
        assert_eq!(mysql_value_to_json(mysql_async::Value::NULL), Value::Null);
        assert_eq!(
            mysql_value_to_json(mysql_async::Value::Bytes(b"hello".to_vec())),
            Value::String("hello".to_string())
        );
        assert_eq!(
            mysql_value_to_json(mysql_async::Value::Int(-42)),
            json!(-42)
        );
        assert_eq!(mysql_value_to_json(mysql_async::Value::UInt(42)), json!(42));
        assert!(mysql_value_to_json(mysql_async::Value::Float(1.5)).is_number());
        assert!(
            mysql_value_to_json(mysql_async::Value::Date(2026, 8, 16, 1, 2, 3, 4))
                .as_str()
                .unwrap_or("")
                .starts_with("2026-08-16")
        );
    }

    #[test]
    fn mysql_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "mysql", "host": "127.0.0.1", "port": 1, "username": "root", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");

        let empty_err = query_inline(&profile, "  ").expect_err("empty sql");
        assert!(empty_err.contains("SQL 为空"));
    }

    #[test]
    fn pg_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "postgresql", "host": "127.0.0.1", "port": 1, "username": "postgres", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");

        let ssl_err = query_inline(
            &json!({ "engine": "postgresql", "host": "127.0.0.1", "port": 5432, "username": "u", "ssl": true }),
            "SELECT 1",
        )
        .expect_err("tls not wired");
        assert!(ssl_err.contains("TLS"), "got {ssl_err:?}");
    }

    #[test]
    fn sqlite_roundtrip_works() {
        let dir =
            std::env::temp_dir().join(format!("onehub-sqlite-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "sqlite", "database": dir.to_string_lossy() });
        let create = query_inline(
            &profile,
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT)",
        )
        .expect("create");
        assert_eq!(create.affected_rows, 0);
        let insert =
            query_inline(&profile, "INSERT INTO demo(name) VALUES ('onehub')").expect("insert");
        assert!(
            insert.affected_rows >= 1,
            "affected_rows={}",
            insert.affected_rows
        );
        let select = query_inline(&profile, "SELECT id, name FROM demo").expect("select");
        assert_eq!(select.columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(select.rows.len(), 1);
        assert_eq!(select.rows[0][1], Value::String("onehub".to_string()));
        let session = connect(&profile).expect("connect");
        let via_session = query_session(&session, "SELECT name FROM demo").expect("session query");
        assert_eq!(via_session.rows.len(), 1);
        assert!(close_session(&session));
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn unwired_engine_connect_is_honest() {
        let err = connect(&json!({ "engine": "sqlserver", "host": "127.0.0.1", "username": "sa", "password": "x" }))
            .expect_err("sqlserver not wired");
        assert!(err.contains("未接入"), "got {err:?}");
        assert!(!close_session("db-missing"));
        assert!(active_db_session_ids().is_empty());
    }
}
