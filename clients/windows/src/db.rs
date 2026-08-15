//! Database workspace framework (navop parity, T018+).
//!
//! Provides the database-engine registry, connection-profile parsing, the
//! `db_connection_*`/`db_query`/`db_exec` command surface (routed by main.rs),
//! and live engine sessions. MySQL (T019) and PostgreSQL (T020) are wired with
//! real connections -> query -> result sets; other engines are wired in later
//! rows. Nothing is faked: unwired engines return a clear recoverable error.

use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::probe;
use mysql_async::prelude::Queryable;
use tokio_postgres::types::Type;
use tokio_postgres::NoTls;
use tokio_util::compat::TokioAsyncWriteCompatExt;

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
    &[
        "mysql",
        "postgresql",
        "sqlite",
        "duckdb",
        "sqlserver",
        "oracle",
        "clickhouse",
        "dm",
        "kingbase",
        "gbase",
        "oceanbase",
        "opengauss",
        "iotdb",
        "redis",
    ]
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

/// Maps an engine to the driver kind used to reach it (the unified extension
/// driver framework). Protocol-compatible engines share a driver; DM/GBase go
/// through a system ODBC driver.
pub fn driver_kind(engine: &str) -> &'static str {
    match engine {
        "mysql" | "oceanbase" => "mysql",
        "postgresql" | "opengauss" | "kingbase" => "postgres",
        "sqlite" => "sqlite",
        "duckdb" => "duckdb",
        "sqlserver" => "tiberius",
        "oracle" => "oci",
        "clickhouse" | "iotdb" => "http",
        "dm" | "gbase" => "odbc",
        "redis" => "redis",
        "mongodb" => "mongodb",
        _ => "external",
    }
}

/// Extension database provider registry: engine -> label -> driver kind ->
/// wired status -> note. This is the surface consumed by the UI engine picker
/// and by later extension-marketplace rows (T051).
pub fn provider_registry() -> Vec<Value> {
    DB_ENGINES
        .iter()
        .map(|(engine, label)| {
            json!({
                "engine": engine,
                "label": label,
                "driver": driver_kind(engine),
                "available": engine_available(engine),
                "note": match driver_kind(engine) {
                    "odbc" => "需要系统 ODBC 驱动（达梦/GBase 等）",
                    "oci" => "需要 Oracle Instant Client (OCI)",
                    "http" => "通过 HTTP 接口",
                    _ => "内置驱动",
                },
            })
        })
        .collect()
}

/// Engine catalog for `db_engine_list` (UI engine picker).
pub fn engine_list() -> Vec<Value> {
    DB_ENGINES
        .iter()
        .map(|(key, label)| {
            json!({
                "engine": key,
                "label": label,
                "driver": driver_kind(key),
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
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub tunnel_rule_id: String,
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
        let proxy_type = profile
            .get("proxy_type")
            .and_then(Value::as_str)
            .unwrap_or("none")
            .to_string();
        let proxy_host = profile
            .get("proxy_host")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let proxy_port = profile
            .get("proxy_port")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u16;
        let proxy_username = profile
            .get("proxy_username")
            .and_then(Value::as_str)
            .map(str::to_string);
        let proxy_password = profile
            .get("proxy_password")
            .and_then(Value::as_str)
            .map(str::to_string);
        let tunnel_rule_id = profile
            .get("tunnel_rule_id")
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
            proxy_type,
            proxy_host,
            proxy_port,
            proxy_username,
            proxy_password,
            tunnel_rule_id,
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
        "mysql" => 3306,
        "oceanbase" => 2881,
        "postgresql" | "opengauss" => 5432,
        "gbase" => 9088,
        "kingbase" => 54321,
        "sqlserver" | "dm" => 1433,
        "oracle" => 1521,
        "clickhouse" => 8123,
        "iotdb" => 18080,
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
                "route": null,
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
            "route": null,
            "message": if path_ok { "本地数据库文件可用" } else { "本地数据库文件不存在（连接时将创建）" },
        });
    }
    let route_result = db_proxy_route(&parsed);
    let route = route_result.as_ref().ok().cloned();
    let label = engine_label(&parsed.engine).unwrap_or(&parsed.engine);
    let timeout = Duration::from_millis(parsed.connect_timeout_ms.max(500));
    let (reachable, latency_ms, message) = match parsed.proxy_type.as_str() {
        "" | "none" => {
            let target = probe::Target {
                host: parsed.host.clone(),
                port: parsed.port,
                username: parsed.username.clone(),
            };
            let probe = probe::probe_tcp(&target, timeout);
            let message = if probe.reachable {
                format!("TCP 可达（{label}）")
            } else {
                "TCP 不可达（端口未监听或被防火墙拦截）".to_string()
            };
            (probe.reachable, probe.latency_ms, message)
        }
        "socks5" | "http" => {
            let started = std::time::Instant::now();
            let proxied = runtime().block_on(proxy_connect(
                &parsed.proxy_config(),
                &parsed.host,
                parsed.port,
                parsed.connect_timeout_ms.max(500),
            ));
            match proxied {
                Ok(_) => {
                    let latency_ms = started.elapsed().as_millis() as u64;
                    (true, Some(latency_ms), format!("代理路由可达（{label}）"))
                }
                Err(error) => (false, None, format!("代理路由不可达：{error}")),
            }
        }
        "ssh" => match &route {
            Some(route_value) => {
                let host = route_value["endpoint"]["host"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let port = route_value["endpoint"]["port"].as_u64().unwrap_or(0) as u16;
                let target = probe::Target {
                    host,
                    port,
                    username: parsed.username.clone(),
                };
                let probe = probe::probe_tcp(&target, timeout);
                let message = if probe.reachable {
                    format!("SSH 隧道端点可达（{label}）")
                } else {
                    "SSH 隧道端点不可达".to_string()
                };
                (probe.reachable, probe.latency_ms, message)
            }
            None => {
                let detail = route_result
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "SSH 隧道路由不可用".to_string());
                (false, None, format!("SSH 隧道路由不可用：{detail}"))
            }
        },
        other => (false, None, format!("未知代理类型：{other}")),
    };
    json!({
        "ok": true,
        "reachable": reachable,
        "latency_ms": latency_ms,
        "engine": parsed.engine,
        "engine_available": engine_available(&parsed.engine),
        "route": route,
        "message": message,
    })
}

/// Parsed proxy settings for a database profile (T038).
#[derive(Debug, Clone, Default)]
pub struct ProxyConfig {
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
}

impl DbProfile {
    /// Extracts the profile's proxy settings.
    pub fn proxy_config(&self) -> ProxyConfig {
        ProxyConfig {
            proxy_type: self.proxy_type.clone(),
            proxy_host: self.proxy_host.clone(),
            proxy_port: self.proxy_port,
            proxy_username: self.proxy_username.clone(),
            proxy_password: self.proxy_password.clone(),
        }
    }
}

/// Resolves a database profile's proxy route to a concrete endpoint. Supports
/// direct, SOCKS5, HTTP CONNECT and SSH-tunnel (reusing the tunnels runtime).
pub fn db_proxy_route(parsed: &DbProfile) -> Result<Value, String> {
    let direct = json!({ "host": parsed.host, "port": parsed.port });
    match parsed.proxy_type.as_str() {
        "" | "none" => Ok(json!({
            "proxy_type": "none",
            "direct": direct,
            "endpoint": direct,
            "via": "direct",
            "note": "直连",
        })),
        "socks5" | "http" => {
            if parsed.proxy_host.trim().is_empty() || parsed.proxy_port == 0 {
                return Err("代理路由配置不完整：缺少代理主机或端口。".to_string());
            }
            let endpoint = json!({ "host": parsed.proxy_host, "port": parsed.proxy_port });
            Ok(json!({
                "proxy_type": parsed.proxy_type,
                "direct": direct,
                "endpoint": endpoint,
                "via": parsed.proxy_type,
                "note": if parsed.proxy_type == "socks5" {
                    "经 SOCKS5 代理"
                } else {
                    "经 HTTP CONNECT 代理"
                },
            }))
        }
        "ssh" => {
            if parsed.tunnel_rule_id.trim().is_empty() {
                return Err("SSH 隧道路由需要 tunnel_rule_id。".to_string());
            }
            match crate::tunnels::tunnel_endpoint(&parsed.tunnel_rule_id) {
                Some((host, port)) => Ok(json!({
                    "proxy_type": "ssh",
                    "direct": direct,
                    "endpoint": { "host": host, "port": port },
                    "via": "ssh-tunnel",
                    "tunnel_rule_id": parsed.tunnel_rule_id,
                    "note": format!("经 SSH 隧道 {host}:{port}"),
                })),
                None => Err(format!(
                    "SSH 隧道未运行：{}。请先启动隧道规则再连接。",
                    parsed.tunnel_rule_id
                )),
            }
        }
        other => Err(format!("未知代理类型：{other}")),
    }
}

/// Opens a TCP stream through a SOCKS5 or HTTP CONNECT proxy and runs the
/// proxy handshake against `target`. Returns the connected (proxied) stream so
/// callers can continue with engine-specific protocols (e.g. tiberius).
/// SOCKS5 (RFC 1928 + RFC 1929) client CONNECT handshake over a connected
/// TCP stream. In-house so the L5 client keeps its abi-c-only path boundary.
async fn socks5_connect_handshake(
    stream: &mut tokio::net::TcpStream,
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
    timeout: Duration,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let handshake = async {
        let methods: Vec<u8> = if username.is_some() {
            vec![0x00, 0x02]
        } else {
            vec![0x00]
        };
        let mut greeting = vec![0x05, methods.len() as u8];
        greeting.extend_from_slice(&methods);
        stream
            .write_all(&greeting)
            .await
            .map_err(|e| format!("SOCKS5 写入失败：{e}"))?;
        let mut selection = [0u8; 2];
        stream
            .read_exact(&mut selection)
            .await
            .map_err(|e| format!("SOCKS5 读取失败：{e}"))?;
        if selection[0] != 0x05 {
            return Err("SOCKS5 版本不匹配".to_string());
        }
        match selection[1] {
            0x00 => {}
            0x02 => {
                let user = username.unwrap_or("");
                let pass = password.unwrap_or("");
                let mut auth = vec![0x01, user.len() as u8];
                auth.extend_from_slice(user.as_bytes());
                auth.push(pass.len() as u8);
                auth.extend_from_slice(pass.as_bytes());
                stream
                    .write_all(&auth)
                    .await
                    .map_err(|e| format!("SOCKS5 认证写入失败：{e}"))?;
                let mut status = [0u8; 2];
                stream
                    .read_exact(&mut status)
                    .await
                    .map_err(|e| format!("SOCKS5 认证读取失败：{e}"))?;
                if status[0] != 0x01 || status[1] != 0x00 {
                    return Err("SOCKS5 认证被拒绝".to_string());
                }
            }
            0xff => return Err("SOCKS5 无可用认证方式".to_string()),
            other => return Err(format!("SOCKS5 未知认证方式：{other}")),
        }
        let host_bytes = target_host.as_bytes();
        if host_bytes.len() > 255 {
            return Err("目标主机名过长".to_string());
        }
        let mut request = vec![0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8];
        request.extend_from_slice(host_bytes);
        request.extend_from_slice(&target_port.to_be_bytes());
        stream
            .write_all(&request)
            .await
            .map_err(|e| format!("SOCKS5 连接请求写入失败：{e}"))?;
        let mut reply = [0u8; 4];
        stream
            .read_exact(&mut reply)
            .await
            .map_err(|e| format!("SOCKS5 连接响应读取失败：{e}"))?;
        if reply[0] != 0x05 {
            return Err("SOCKS5 响应版本不匹配".to_string());
        }
        if reply[1] != 0x00 {
            return Err(format!("SOCKS5 连接被拒绝（代码 {}）", reply[1]));
        }
        let addr_len = match reply[3] {
            0x01 => 4,
            0x04 => 16,
            0x03 => {
                let mut len = [0u8; 1];
                stream
                    .read_exact(&mut len)
                    .await
                    .map_err(|e| format!("SOCKS5 读取失败：{e}"))?;
                len[0] as usize
            }
            other => return Err(format!("SOCKS5 未知地址类型：{other}")),
        };
        let mut rest = vec![0u8; addr_len + 2];
        stream
            .read_exact(&mut rest)
            .await
            .map_err(|e| format!("SOCKS5 读取失败：{e}"))?;
        Ok::<(), String>(())
    };
    match tokio::time::timeout(timeout, handshake).await {
        Ok(result) => result,
        Err(_) => Err(format!("SOCKS5 握手超时（{}ms）", timeout.as_millis())),
    }
}

/// HTTP CONNECT proxy handshake (CONNECT host:port HTTP/1.1 + 2xx check).
async fn http_connect_handshake(
    stream: &mut tokio::net::TcpStream,
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
    timeout: Duration,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let handshake = async {
        let authorization = username.map(|user| {
            let credentials = format!("{}:{}", user, password.unwrap_or(""));
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes())
            )
        });
        let authority = format!("{target_host}:{target_port}");
        let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
        if let Some(auth) = authorization {
            request.push_str(&format!("Proxy-Authorization: {auth}\r\n"));
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| format!("HTTP CONNECT 写入失败：{e}"))?;
        let mut header = Vec::new();
        let mut buf = [0u8; 1];
        loop {
            stream
                .read_exact(&mut buf)
                .await
                .map_err(|e| format!("HTTP CONNECT 读取失败：{e}"))?;
            header.push(buf[0]);
            if header.len() >= 4 && &header[header.len() - 4..] == b"\r\n\r\n" {
                break;
            }
            if header.len() > 16 * 1024 {
                return Err("HTTP CONNECT 响应头过大".to_string());
            }
        }
        let text = String::from_utf8_lossy(&header);
        let status_line = text.lines().next().unwrap_or("");
        let code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(0);
        if !(200..300).contains(&code) {
            return Err(format!("HTTP CONNECT 被拒绝（HTTP {code}）"));
        }
        Ok::<(), String>(())
    };
    match tokio::time::timeout(timeout, handshake).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "HTTP CONNECT 握手超时（{}ms）",
            timeout.as_millis()
        )),
    }
}

/// Opens a TCP stream through a SOCKS5 or HTTP CONNECT proxy and runs the
/// proxy handshake against `target`. Returns the connected (proxied) stream so
/// callers can continue with engine-specific protocols (e.g. tiberius).
async fn proxy_connect(
    config: &ProxyConfig,
    target_host: &str,
    target_port: u16,
    timeout_ms: u64,
) -> Result<tokio::net::TcpStream, String> {
    let mut stream =
        tokio::net::TcpStream::connect((config.proxy_host.as_str(), config.proxy_port))
            .await
            .map_err(|e| format!("代理连接失败：{e}"))?;
    let timeout = Duration::from_millis(timeout_ms.max(500));
    match config.proxy_type.as_str() {
        "socks5" => {
            socks5_connect_handshake(
                &mut stream,
                target_host,
                target_port,
                config.proxy_username.as_deref(),
                config.proxy_password.as_deref(),
                timeout,
            )
            .await?;
        }
        "http" => {
            http_connect_handshake(
                &mut stream,
                target_host,
                target_port,
                config.proxy_username.as_deref(),
                config.proxy_password.as_deref(),
                timeout,
            )
            .await?;
        }
        other => return Err(format!("未知代理类型：{other}")),
    }
    Ok(stream)
}
/// Concrete tiberius client stream:/// Concrete tiberius client stream: tokio TcpStream wrapped in tokio-util compat.
type MsSqlClient = tiberius::Client<tokio_util::compat::Compat<tokio::net::TcpStream>>;

/// A live engine connection held by a session.
enum EngineConnection {
    MySql(mysql_async::Pool),
    Postgres(std::sync::Arc<tokio_postgres::Client>),
    Sqlite(std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>),
    DuckDb(std::sync::Arc<std::sync::Mutex<duckdb::Connection>>),
    SqlServer(std::sync::Arc<tokio::sync::Mutex<MsSqlClient>>),
    Oracle(std::sync::Arc<std::sync::Mutex<oracle::Connection>>),
    ClickHouse(String),
    /// ODBC connection string for extension engines that go through a
    /// system ODBC driver (达梦 DM, later GBase).
    Odbc(String),
    /// IoTDB HTTP REST endpoint (base URL); stateless like ClickHouse.
    IotDb(String),
    /// Redis async connection manager (T039).
    Redis(std::sync::Arc<tokio::sync::Mutex<redis::aio::ConnectionManager>>),
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

/// Connects an Oracle client (oracle crate; requires OCI libraries at
/// runtime, so hosts without Oracle Instant Client fail gracefully).
fn oracle_connect(parsed: &DbProfile) -> Result<oracle::Connection, String> {
    let service = if parsed.database.is_empty() {
        "ORCL"
    } else {
        parsed.database.as_str()
    };
    let connect_string = format!("//{}:{}/{}", parsed.host, parsed.port, service);
    oracle::Connection::connect(
        parsed.username.as_str(),
        parsed.password.as_deref().unwrap_or(""),
        connect_string.as_str(),
    )
    .map_err(|e| format!("Oracle 连接失败：{e}"))
}

/// Converts an Oracle cell into a JSON value via a type cascade.
fn oracle_cell_to_json(row: &oracle::Row, index: usize) -> Value {
    if let Ok(Some(v)) = row.get::<usize, Option<bool>>(index) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.get::<usize, Option<i64>>(index) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.get::<usize, Option<f64>>(index) {
        return serde_json::Number::from_f64(v)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if let Ok(Some(v)) = row.get::<usize, Option<String>>(index) {
        return Value::String(v);
    }
    if let Ok(Some(v)) = row.get::<usize, Option<chrono::NaiveDate>>(index) {
        return Value::String(v.to_string());
    }
    if let Ok(Some(v)) = row.get::<usize, Option<chrono::NaiveDateTime>>(index) {
        return Value::String(v.to_string());
    }
    if let Ok(Some(v)) = row.get::<usize, Option<Vec<u8>>>(index) {
        return Value::String(String::from_utf8_lossy(&v).to_string());
    }
    Value::Null
}

/// Runs a statement against an Oracle connection (synchronous). DML affected
/// rows are not exposed by the oracle crate's Statement API; SELECT
/// columns/rows are fully real.
fn oracle_run(conn: &oracle::Connection, sql: &str) -> Result<QueryOutcome, String> {
    let mut stmt = conn
        .statement(sql)
        .build()
        .map_err(|e| format!("SQL 准备失败：{e}"))?;
    match stmt.query(&[]) {
        Ok(result_set) => {
            let mut columns: Vec<String> = Vec::new();
            let mut result_rows = Vec::new();
            for row_result in result_set {
                let row = row_result.map_err(|e| format!("读取结果失败：{e}"))?;
                if columns.is_empty() {
                    columns = row
                        .column_info()
                        .iter()
                        .map(|info| info.name().to_string())
                        .collect();
                }
                let mut values = Vec::new();
                for index in 0..columns.len() {
                    values.push(oracle_cell_to_json(&row, index));
                }
                result_rows.push(values);
            }
            Ok(QueryOutcome {
                columns,
                rows: result_rows,
                affected_rows: 0,
            })
        }
        Err(query_err) => {
            // DML/DDL path (query rejected the statement).
            stmt.execute(&[])
                .map_err(|_| format!("SQL 执行失败：{query_err}"))?;
            Ok(QueryOutcome {
                columns: Vec::new(),
                rows: Vec::new(),
                affected_rows: 0,
            })
        }
    }
}
/// Builds the ODBC connection string for extension engines that go through a
/// system ODBC driver (达梦 DM / GBase 8s; the driver must be installed).
fn odbc_conn_string(engine: &str, parsed: &DbProfile) -> String {
    let driver = match engine {
        "gbase" => "GBase 8s ODBC DRIVER",
        _ => "DM8 ODBC DRIVER",
    };
    format!(
        "Driver={{{driver}}};Server={};Port={};UID={};PWD={};DATABASE={}",
        parsed.host,
        parsed.port,
        parsed.username,
        parsed.password.as_deref().unwrap_or(""),
        parsed.database
    )
}

/// Runs a statement through a system ODBC driver (used by DM). Without the
/// driver installed the connection fails gracefully with the ODBC diagnostic.
fn odbc_query(parsed: &DbProfile, sql: &str) -> Result<QueryOutcome, String> {
    if sql.trim().is_empty() {
        return Err("SQL 为空。".to_string());
    }
    let conn_str = odbc_conn_string(&parsed.engine, parsed);
    let env = odbc::Environment::new().map_err(|e| format!("ODBC 环境创建失败：{e:?}"))?;
    let conn = env
        .connect_with_connection_string(&conn_str)
        .map_err(|e| format!("ODBC 连接失败：{e:?}（请安装达梦 ODBC 驱动并核对连接串/服务名）"))?;
    let stmt =
        odbc::Statement::with_parent(&conn).map_err(|e| format!("ODBC 语句创建失败：{e:?}"))?;
    let state = stmt
        .exec_direct(sql)
        .map_err(|e| format!("SQL 执行失败：{e:?}"))?;
    match state {
        odbc::ResultSetState::NoData(stmt) => {
            let affected = stmt.affected_row_count().unwrap_or(0).max(0) as u64;
            Ok(QueryOutcome {
                columns: Vec::new(),
                rows: Vec::new(),
                affected_rows: affected,
            })
        }
        odbc::ResultSetState::Data(mut stmt) => {
            let col_count = stmt.num_result_cols().unwrap_or(0).max(0) as usize;
            let columns: Vec<String> = (1..=col_count)
                .map(|index| {
                    stmt.describe_col(index as u16)
                        .map(|descriptor| descriptor.name)
                        .unwrap_or_default()
                })
                .collect();
            let mut result_rows = Vec::new();
            while let Some(mut cursor) = stmt.fetch().map_err(|e| format!("读取行失败：{e:?}"))?
            {
                let mut values = Vec::new();
                for index in 1..=col_count {
                    let value: Option<String> = cursor
                        .get_data(index as u16)
                        .map_err(|e| format!("读取列失败：{e:?}"))?;
                    values.push(value.map(Value::String).unwrap_or(Value::Null));
                }
                result_rows.push(values);
            }
            Ok(QueryOutcome {
                columns,
                rows: result_rows,
                affected_rows: 0,
            })
        }
    }
}

/// Runs a statement against Apache IoTDB through its HTTP REST API (login for
/// a token, then POST /query). Without a server the request fails gracefully.
fn iotdb_query(parsed: &DbProfile, sql: &str) -> Result<QueryOutcome, String> {
    if sql.trim().is_empty() {
        return Err("SQL 为空。".to_string());
    }
    let base = format!("http://{}:{}", parsed.host, parsed.port);
    let login_response = ureq::post(&format!("{base}/login"))
        .timeout(Duration::from_millis(parsed.connect_timeout_ms.max(1000)))
        .set("Content-Type", "application/json")
        .send_string(
            &serde_json::json!({
                "username": parsed.username,
                "password": parsed.password.as_deref().unwrap_or(""),
            })
            .to_string(),
        )
        .map_err(|e| format!("IoTDB 登录失败：{e}"))?;
    let login_text = login_response
        .into_string()
        .map_err(|e| format!("IoTDB 登录响应读取失败：{e}"))?;
    let login_body: Value =
        serde_json::from_str(&login_text).map_err(|e| format!("IoTDB 登录响应解析失败：{e}"))?;
    let token = login_body
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("");
    let response = ureq::post(&format!("{base}/query"))
        .set("Authorization", token)
        .timeout(Duration::from_millis(parsed.connect_timeout_ms.max(1000)))
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({ "sql": sql }).to_string())
        .map_err(|e| format!("IoTDB 查询失败：{e}"))?;
    let response_text = response
        .into_string()
        .map_err(|e| format!("IoTDB 查询响应读取失败：{e}"))?;
    let body: Value =
        serde_json::from_str(&response_text).map_err(|e| format!("IoTDB 查询响应解析失败：{e}"))?;
    let columns: Vec<String> = body
        .get("columns")
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let values: Vec<Vec<Value>> = body
        .get("values")
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|row| row.as_array().cloned())
                .collect()
        })
        .unwrap_or_default();
    Ok(QueryOutcome {
        columns,
        rows: values,
        affected_rows: 0,
    })
}

/// Builds the ClickHouse HTTP base URL and runs a statement over HTTP.
///
/// SELECT-like statements get "FORMAT JSONEachRow" appended so results are
/// returned as JSON lines; DML/DDL return an empty body (affected rows are not
/// reported by the HTTP protocol and stay 0).
fn clickhouse_query(parsed: &DbProfile, sql: &str) -> Result<QueryOutcome, String> {
    if sql.trim().is_empty() {
        return Err("SQL 为空。".to_string());
    }
    let scheme = if parsed.ssl { "https" } else { "http" };
    let base = format!("{scheme}://{}:{}", parsed.host, parsed.port);
    let upper = sql.trim().to_uppercase();
    let leading = upper.trim_start();
    let is_query = ["SELECT", "SHOW", "DESCRIBE", "DESC", "EXISTS", "WITH"]
        .iter()
        .any(|keyword| leading.starts_with(keyword));
    let mut statement = sql.trim().to_string();
    if is_query && !upper.contains("FORMAT") {
        statement.push_str(" FORMAT JSONEachRow");
    }
    let response = ureq::post(&base)
        .set("X-ClickHouse-User", parsed.username.as_str())
        .set("X-ClickHouse-Key", parsed.password.as_deref().unwrap_or(""))
        .set("Content-Type", "text/plain; charset=utf-8")
        .timeout(Duration::from_millis(parsed.connect_timeout_ms.max(1000)))
        .send_string(&statement)
        .map_err(|e| format!("ClickHouse 连接失败：{e}"))?;
    let text = response
        .into_string()
        .map_err(|e| format!("ClickHouse 响应失败：{e}"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "Ok." {
        return Ok(QueryOutcome {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: 0,
        });
    }
    let mut columns: Vec<String> = Vec::new();
    let mut result_rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(line).map_err(|e| format!("ClickHouse 结果解析失败：{e}"))?;
        match value {
            Value::Object(map) => {
                if columns.is_empty() {
                    columns = map.keys().cloned().collect();
                }
                let mut values = Vec::new();
                for column in &columns {
                    values.push(map.get(column).cloned().unwrap_or(Value::Null));
                }
                result_rows.push(values);
            }
            other => {
                if columns.is_empty() {
                    columns.push("value".to_string());
                }
                result_rows.push(vec![other]);
            }
        }
    }
    Ok(QueryOutcome {
        columns,
        rows: result_rows,
        affected_rows: 0,
    })
}

/// Connects a SQL Server client (tiberius + rustls). Windows integrated auth
/// is used when the username is blank, otherwise SQL Server auth.
fn mssql_connect(parsed: &DbProfile) -> Result<MsSqlClient, String> {
    runtime().block_on(async {
        let tcp = if matches!(parsed.proxy_type.as_str(), "socks5" | "http") {
            proxy_connect(
                &parsed.proxy_config(),
                &parsed.host,
                parsed.port,
                parsed.connect_timeout_ms.max(500),
            )
            .await
            .map_err(|e| format!("SQL Server 代理连接失败：{e}"))?
        } else {
            tokio::net::TcpStream::connect((parsed.host.as_str(), parsed.port))
                .await
                .map_err(|e| format!("SQL Server 连接失败：{e}"))?
        };
        tcp.set_nodelay(true)
            .map_err(|e| format!("SQL Server 配置失败：{e}"))?;
        let mut config = tiberius::Config::new();
        config.host(&parsed.host);
        config.port(parsed.port);
        if parsed.username.trim().is_empty() {
            config.authentication(tiberius::AuthMethod::Integrated);
        } else {
            config.authentication(tiberius::AuthMethod::sql_server(
                parsed.username.as_str(),
                parsed.password.as_deref().unwrap_or(""),
            ));
        }
        if !parsed.database.is_empty() {
            config.database(&parsed.database);
        }
        if parsed.ssl {
            config.encryption(tiberius::EncryptionLevel::Required);
        } else {
            config.encryption(tiberius::EncryptionLevel::NotSupported);
        }
        config.trust_cert();
        let client = tiberius::Client::connect(config, tcp.compat_write())
            .await
            .map_err(|e| format!("SQL Server 认证失败：{e}"))?;
        Ok::<MsSqlClient, String>(client)
    })
}

/// Converts a SQL Server cell into a JSON value via a type cascade.
fn mssql_cell_to_json(row: &tiberius::Row, index: usize) -> Value {
    if let Ok(Some(v)) = row.try_get::<bool, usize>(index) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<i32, usize>(index) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<i64, usize>(index) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<f64, usize>(index) {
        return serde_json::Number::from_f64(v)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if let Ok(Some(v)) = row.try_get::<&str, usize>(index) {
        return Value::String(v.to_string());
    }
    if let Ok(Some(v)) = row.try_get::<chrono::NaiveDate, usize>(index) {
        return Value::String(v.to_string());
    }
    if let Ok(Some(v)) = row.try_get::<chrono::NaiveTime, usize>(index) {
        return Value::String(v.to_string());
    }
    if let Ok(Some(v)) = row.try_get::<chrono::NaiveDateTime, usize>(index) {
        return Value::String(v.to_string());
    }
    if let Ok(Some(v)) = row.try_get::<chrono::DateTime<chrono::Utc>, usize>(index) {
        return Value::String(v.to_string());
    }
    Value::Null
}

/// Runs a statement against a SQL Server client. DML affected rows are not
/// exposed through tiberius QueryStream (only ExecuteResult), so DML reports 0
/// affected rows; SELECT columns/rows are fully real.
async fn mssql_run(client: &mut MsSqlClient, sql: &str) -> Result<QueryOutcome, String> {
    let mut stream = client
        .query(sql, &[])
        .await
        .map_err(|e| format!("SQL 执行失败：{e}"))?;
    let columns: Vec<String> = stream
        .columns()
        .await
        .map_err(|e| format!("读取列信息失败：{e}"))?
        .map(|cols| cols.iter().map(|col| col.name().to_string()).collect())
        .unwrap_or_default();
    if columns.is_empty() {
        return Ok(QueryOutcome {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: 0,
        });
    }
    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("查询失败：{e}"))?;
    let mut result_rows = Vec::new();
    for row in rows {
        let mut values = Vec::new();
        for index in 0..columns.len() {
            values.push(mssql_cell_to_json(&row, index));
        }
        result_rows.push(values);
    }
    Ok(QueryOutcome {
        columns,
        rows: result_rows,
        affected_rows: 0,
    })
}

/// Opens a DuckDB database (file path or :memory:).
fn duckdb_open(parsed: &DbProfile) -> Result<duckdb::Connection, String> {
    let path = if parsed.database.is_empty() {
        ":memory:".to_string()
    } else {
        parsed.database.clone()
    };
    duckdb::Connection::open(&path).map_err(|e| format!("DuckDB 打开失败：{e}"))
}

/// Converts a duckdb value into a JSON value.
fn duckdb_value_to_json(value: duckdb::types::Value) -> Value {
    use duckdb::types::Value as Dv;
    match value {
        Dv::Null => Value::Null,
        Dv::Boolean(b) => serde_json::json!(b),
        Dv::TinyInt(i) => serde_json::json!(i),
        Dv::SmallInt(i) => serde_json::json!(i),
        Dv::Int(i) => serde_json::json!(i),
        Dv::BigInt(i) => serde_json::json!(i),
        Dv::UTinyInt(i) => serde_json::json!(i),
        Dv::USmallInt(i) => serde_json::json!(i),
        Dv::UInt(i) => serde_json::json!(i),
        Dv::UBigInt(i) => serde_json::json!(i),
        Dv::HugeInt(i) => serde_json::Number::from_i128(i)
            .map(Value::Number)
            .unwrap_or(Value::String(i.to_string())),
        Dv::UHugeInt(i) => serde_json::Number::from_u128(i)
            .map(Value::Number)
            .unwrap_or(Value::String(i.to_string())),
        Dv::Float(f) => serde_json::Number::from_f64(f as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Dv::Double(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Dv::Decimal(d) => Value::String(d.to_string()),
        Dv::Timestamp(_, i) => Value::String(i.to_string()),
        Dv::Text(t) => Value::String(t),
        Dv::Enum(t) => Value::String(t),
        Dv::Blob(b) | Dv::Geometry(b) => Value::String(String::from_utf8_lossy(&b).to_string()),
        Dv::List(items) | Dv::Array(items) => {
            Value::Array(items.into_iter().map(duckdb_value_to_json).collect())
        }
        Dv::Struct(map) => {
            let mut object = serde_json::Map::new();
            for (key, value) in map.iter() {
                object.insert(key.clone(), duckdb_value_to_json(value.clone()));
            }
            Value::Object(object)
        }
        Dv::Map(map) => {
            let mut object = serde_json::Map::new();
            for (key, value) in map.iter() {
                let key_text = match key {
                    Dv::Text(t) => t.clone(),
                    other => format!("{other:?}"),
                };
                object.insert(key_text, duckdb_value_to_json(value.clone()));
            }
            Value::Object(object)
        }
        Dv::Union(inner) => duckdb_value_to_json(*inner),
        _ => Value::Null,
    }
}

/// Runs a statement against a DuckDB connection (synchronous).
///
/// DuckDB resolves columns at execution time and reports DML/DDL as a single
/// "Count" result column, so the runner executes once, inspects the executed
/// statement, and reads rows (or the affected count) accordingly.
fn duckdb_run(conn: &duckdb::Connection, sql: &str) -> Result<QueryOutcome, String> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("SQL 准备失败：{e}"))?;
    let mut rows = stmt.query([]).map_err(|e| format!("查询失败：{e}"))?;
    let statement = rows
        .as_ref()
        .ok_or_else(|| "DuckDB 未返回结果。".to_string())?;
    let column_count = statement.column_count();
    if column_count == 0 {
        return Ok(QueryOutcome {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: 0,
        });
    }
    let first_column = statement.column_name(0).ok().cloned().unwrap_or_default();
    if column_count == 1 && first_column == "Count" {
        let mut affected: u64 = 0;
        if let Some(row) = rows.next().map_err(|e| format!("读取结果失败：{e}"))? {
            affected = row.get::<usize, i64>(0).unwrap_or(0).max(0) as u64;
        }
        return Ok(QueryOutcome {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: affected,
        });
    }
    let columns: Vec<String> = (0..column_count)
        .filter_map(|index| statement.column_name(index).ok().cloned())
        .collect();
    let mut result_rows = Vec::new();
    while let Some(row) = rows.next().map_err(|e| format!("读取结果失败：{e}"))? {
        let mut values = Vec::new();
        for index in 0..column_count {
            let value = row
                .get::<usize, duckdb::types::Value>(index)
                .unwrap_or(duckdb::types::Value::Null);
            values.push(duckdb_value_to_json(value));
        }
        result_rows.push(values);
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
        "mysql" | "oceanbase" => {
            let pool = build_mysql_pool(&parsed)?;
            runtime().block_on(mysql_run(&pool, sql))
        }
        "postgresql" | "kingbase" | "opengauss" => {
            if parsed.ssl {
                return Err(
                    "PostgreSQL/Kingbase/openGauss TLS 暂未接入（当前为明文 TCP）。".to_string(),
                );
            }
            let client = pg_connect(&parsed)?;
            runtime().block_on(pg_run(&client, sql))
        }
        "sqlite" => {
            let conn = sqlite_open(&parsed)?;
            sqlite_run(&conn, sql)
        }
        "duckdb" => {
            let conn = duckdb_open(&parsed)?;
            duckdb_run(&conn, sql)
        }
        "sqlserver" => {
            let mut client = mssql_connect(&parsed)?;
            runtime().block_on(mssql_run(&mut client, sql))
        }
        "oracle" => {
            let conn = oracle_connect(&parsed)?;
            oracle_run(&conn, sql)
        }
        "clickhouse" => clickhouse_query(&parsed, sql),
        "dm" | "gbase" => odbc_query(&parsed, sql),
        "iotdb" => iotdb_query(&parsed, sql),
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
            Some(EngineConnection::DuckDb(conn)) => {
                EngineConnection::DuckDb(std::sync::Arc::clone(conn))
            }
            Some(EngineConnection::SqlServer(conn)) => {
                EngineConnection::SqlServer(std::sync::Arc::clone(conn))
            }
            Some(EngineConnection::Oracle(conn)) => {
                EngineConnection::Oracle(std::sync::Arc::clone(conn))
            }
            Some(EngineConnection::ClickHouse(base)) => EngineConnection::ClickHouse(base.clone()),
            Some(EngineConnection::Odbc(conn_str)) => EngineConnection::Odbc(conn_str.clone()),
            Some(EngineConnection::IotDb(base)) => EngineConnection::IotDb(base.clone()),
            Some(EngineConnection::Redis(conn)) => {
                EngineConnection::Redis(std::sync::Arc::clone(conn))
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
        EngineConnection::DuckDb(conn) => {
            let guard = conn.lock().expect("duckdb session lock");
            duckdb_run(&guard, sql)
        }
        EngineConnection::SqlServer(conn) => {
            let mut guard = runtime().block_on(async { conn.lock().await });
            runtime().block_on(mssql_run(&mut guard, sql))
        }
        EngineConnection::Oracle(conn) => {
            let guard = conn.lock().expect("oracle session lock");
            oracle_run(&guard, sql)
        }
        EngineConnection::ClickHouse(_) => {
            let profile = {
                let guard = sessions_map().lock().expect("db sessions lock");
                guard
                    .as_ref()
                    .and_then(|m| m.get(session_id))
                    .map(|s| s.profile.clone())
                    .ok_or_else(|| "数据库会话不存在。".to_string())?
            };
            let parsed = DbProfile::parse(&profile)?;
            clickhouse_query(&parsed, sql)
        }
        EngineConnection::Odbc(_) => {
            let profile = {
                let guard = sessions_map().lock().expect("db sessions lock");
                guard
                    .as_ref()
                    .and_then(|m| m.get(session_id))
                    .map(|s| s.profile.clone())
                    .ok_or_else(|| "数据库会话不存在。".to_string())?
            };
            let parsed = DbProfile::parse(&profile)?;
            odbc_query(&parsed, sql)
        }
        EngineConnection::Redis(conn) => {
            let mut guard = runtime().block_on(async { conn.lock().await });
            redis_run(&mut guard, sql)
        }
        EngineConnection::IotDb(_) => {
            let profile = {
                let guard = sessions_map().lock().expect("db sessions lock");
                guard
                    .as_ref()
                    .and_then(|m| m.get(session_id))
                    .map(|s| s.profile.clone())
                    .ok_or_else(|| "数据库会话不存在。".to_string())?
            };
            let parsed = DbProfile::parse(&profile)?;
            iotdb_query(&parsed, sql)
        }
    }
}

/// Opens a database session for a wired engine. MySQL/PostgreSQL verify the
/// connection with a real handshake; unwired engines return a clear error.
pub fn connect(profile: &Value) -> Result<String, String> {
    let mut parsed = DbProfile::parse(profile)?;
    if parsed.proxy_type == "ssh" {
        let route = db_proxy_route(&parsed)?;
        parsed.host = route["endpoint"]["host"].as_str().unwrap_or("").to_string();
        parsed.port = route["endpoint"]["port"].as_u64().unwrap_or(0) as u16;
    }
    if !engine_available(&parsed.engine) {
        let label = engine_label(&parsed.engine).unwrap_or(&parsed.engine);
        return Err(format!(
            "数据库引擎未接入：{}（{label}），真实连接将在后续版本提供",
            parsed.engine
        ));
    }
    let connection = match parsed.engine.as_str() {
        "mysql" | "oceanbase" => EngineConnection::MySql(build_mysql_pool(&parsed)?),
        "postgresql" | "kingbase" | "opengauss" => {
            if parsed.ssl {
                return Err(
                    "PostgreSQL/Kingbase/openGauss TLS 暂未接入（当前为明文 TCP）。".to_string(),
                );
            }
            EngineConnection::Postgres(pg_connect(&parsed)?)
        }
        "sqlite" => {
            let conn = sqlite_open(&parsed)?;
            EngineConnection::Sqlite(std::sync::Arc::new(std::sync::Mutex::new(conn)))
        }
        "duckdb" => {
            let conn = duckdb_open(&parsed)?;
            EngineConnection::DuckDb(std::sync::Arc::new(std::sync::Mutex::new(conn)))
        }
        "sqlserver" => {
            let client = mssql_connect(&parsed)?;
            EngineConnection::SqlServer(std::sync::Arc::new(tokio::sync::Mutex::new(client)))
        }
        "oracle" => {
            let conn = oracle_connect(&parsed)?;
            EngineConnection::Oracle(std::sync::Arc::new(std::sync::Mutex::new(conn)))
        }
        "clickhouse" => {
            let scheme = if parsed.ssl { "https" } else { "http" };
            EngineConnection::ClickHouse(format!("{scheme}://{}:{}", parsed.host, parsed.port))
        }
        "dm" | "gbase" => EngineConnection::Odbc(odbc_conn_string(&parsed.engine, &parsed)),
        "iotdb" => EngineConnection::IotDb(format!("http://{}:{}", parsed.host, parsed.port)),
        "redis" => EngineConnection::Redis(std::sync::Arc::new(tokio::sync::Mutex::new(
            redis_connect(&parsed)?,
        ))),
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
        EngineConnection::DuckDb(conn) => {
            let guard = conn.lock().expect("duckdb session lock");
            guard
                .query_row("SELECT 1", [], |_| Ok(()))
                .map_err(|e| format!("DuckDB 连接失败：{e}"))?;
        }
        EngineConnection::SqlServer(client) => {
            let mut guard = runtime().block_on(async { client.lock().await });
            runtime()
                .block_on(guard.query("SELECT 1", &[]))
                .map_err(|e| format!("SQL Server 连接失败：{e}"))?;
        }
        EngineConnection::Oracle(conn) => {
            let guard = conn.lock().expect("oracle session lock");
            oracle_run(&guard, "SELECT 1 FROM dual")
                .map_err(|e| format!("Oracle 连接失败：{e}"))?;
        }
        EngineConnection::ClickHouse(_) => {
            clickhouse_query(&parsed, "SELECT 1")
                .map_err(|e| format!("ClickHouse 连接失败：{e}"))?;
        }
        EngineConnection::Odbc(_) => {
            odbc_query(&parsed, "SELECT 1").map_err(|e| format!("ODBC 连接失败：{e}"))?;
        }
        EngineConnection::IotDb(_) => {
            iotdb_query(&parsed, "SHOW VERSION").map_err(|e| format!("IoTDB 连接失败：{e}"))?;
        }
        EngineConnection::Redis(conn) => {
            let mut guard = runtime().block_on(async { conn.lock().await });
            redis_run(&mut guard, "PING").map_err(|e| format!("Redis 连接失败：{e}"))?;
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

/// Returns the catalog SQL used by the cross-engine object browser. ODBC
/// engines (dm/gbase) reuse the Oracle/Informix-style catalogs; empty means
/// the engine has no catalog query (handled by the caller).
pub fn object_catalog_sql(engine: &str) -> &'static str {
    match engine {
        "mysql" | "oceanbase" => "SELECT table_name AS name, CASE WHEN table_type='VIEW' THEN 'view' ELSE 'table' END AS kind FROM information_schema.tables WHERE table_schema = DATABASE() UNION ALL SELECT DISTINCT index_name, 'index' FROM information_schema.statistics WHERE table_schema = DATABASE()",
        "postgresql" | "kingbase" | "opengauss" => "SELECT tablename AS name, 'table' AS kind FROM pg_tables WHERE schemaname NOT IN ('pg_catalog','information_schema') UNION ALL SELECT viewname, 'view' FROM pg_views WHERE schemaname NOT IN ('pg_catalog','information_schema') UNION ALL SELECT indexname, 'index' FROM pg_indexes WHERE schemaname NOT IN ('pg_catalog','information_schema')",
        "sqlite" => "SELECT name, type AS kind FROM sqlite_master WHERE type IN ('table','view','index') AND name NOT LIKE 'sqlite_%'",
        "duckdb" => "SELECT table_name AS name, CASE WHEN table_type='VIEW' THEN 'view' ELSE 'table' END AS kind FROM information_schema.tables WHERE table_schema NOT IN ('information_schema','pg_catalog','temp')",
        "sqlserver" => "SELECT TABLE_NAME AS name, CASE WHEN TABLE_TYPE='VIEW' THEN 'view' ELSE 'table' END AS kind FROM INFORMATION_SCHEMA.TABLES UNION ALL SELECT name, 'index' FROM sys.indexes WHERE type > 0 AND name IS NOT NULL UNION ALL SELECT name, 'procedure' FROM sys.procedures",
        "oracle" | "dm" => "SELECT table_name AS name, 'table' AS kind FROM all_tables UNION ALL SELECT view_name, 'view' FROM all_views UNION ALL SELECT object_name, 'index' FROM all_objects WHERE object_type='INDEX' UNION ALL SELECT object_name, 'procedure' FROM all_procedures",
        "gbase" => "SELECT tabname AS name, 'table' AS kind FROM systables WHERE tabid > 99",
        "clickhouse" => "SELECT name, CASE WHEN engine='View' THEN 'view' ELSE 'table' END AS kind FROM system.tables WHERE database = currentDatabase()",
        "iotdb" => "SHOW TIMESERIES",
        _ => "",
    }
}

/// Engines whose SQL supports the LIMIT/OFFSET pagination syntax.
pub fn engine_supports_limit(engine: &str) -> bool {
    matches!(
        engine,
        "mysql"
            | "oceanbase"
            | "postgresql"
            | "kingbase"
            | "opengauss"
            | "sqlite"
            | "duckdb"
            | "clickhouse"
    )
}

/// Runs EXPLAIN for a session's engine. Engines without a plain EXPLAIN
/// statement return a clear, honest error.
pub fn explain(session_id: &str, sql: &str) -> Result<QueryOutcome, String> {
    let engine = {
        let guard = sessions_map().lock().expect("db sessions lock");
        guard
            .as_ref()
            .and_then(|m| m.get(session_id))
            .map(|s| s.engine.clone())
            .ok_or_else(|| "数据库会话不存在。".to_string())?
    };
    let prefix = match engine.as_str() {
        "mysql" | "oceanbase" | "postgresql" | "kingbase" | "opengauss" | "duckdb"
        | "clickhouse" => "EXPLAIN",
        "sqlite" => "EXPLAIN QUERY PLAN",
        _ => {
            let label = engine_label(&engine).unwrap_or(&engine);
            return Err(format!("该引擎暂不支持 EXPLAIN 查看（{label}）。"));
        }
    };
    let explain_sql = format!("{prefix} {sql}");
    query_session(session_id, &explain_sql)
}

/// Lists database objects (tables/views/indexes/procedures) for a session.
pub fn list_objects(session_id: &str, kind_filter: Option<&str>) -> Result<Vec<Value>, String> {
    let engine = {
        let guard = sessions_map().lock().expect("db sessions lock");
        guard
            .as_ref()
            .and_then(|m| m.get(session_id))
            .map(|s| s.engine.clone())
            .ok_or_else(|| "数据库会话不存在。".to_string())?
    };
    let sql = object_catalog_sql(&engine);
    if sql.is_empty() {
        return Ok(Vec::new());
    }
    let outcome = query_session(session_id, sql)?;
    let mut objects = Vec::new();
    for row in outcome.rows {
        let name = row
            .first()
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let kind = row
            .get(1)
            .and_then(Value::as_str)
            .unwrap_or("table")
            .to_string();
        if kind_filter.map(|filter| kind != filter).unwrap_or(false) {
            continue;
        }
        objects.push(json!({ "name": name, "kind": kind }));
    }
    Ok(objects)
}

/// Quotes a CSV field when needed (commas, quotes, newlines).
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Splits one CSV line into fields, honoring double-quoted segments.
fn split_csv_line(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == ',' {
            fields.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(ch);
        }
    }
    if in_quotes {
        return Err("CSV 引号未闭合。".to_string());
    }
    fields.push(current.trim().to_string());
    Ok(fields)
}

/// Serializes a query outcome to CSV text (header + rows).
fn rows_to_csv(columns: &[String], rows: &[Vec<Value>]) -> String {
    let mut out = columns
        .iter()
        .map(|column| csv_field(column))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for row in rows {
        let fields = row
            .iter()
            .map(|value| match value {
                Value::Null => String::new(),
                Value::String(text) => csv_field(text),
                other => csv_field(&other.to_string()),
            })
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&fields);
        out.push('\n');
    }
    out
}

/// Converts a JSON value into a SQL literal.
fn value_to_sql_literal(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::Number(number) => number.to_string(),
        Value::String(text) => format!("'{}'", text.replace('\'', "''")),
        _ => "NULL".to_string(),
    }
}

/// Builds INSERT statements for a table from columns + rows.
fn build_inserts(table: &str, columns: &[String], rows: &[Vec<Value>]) -> String {
    let column_list = columns
        .iter()
        .map(|column| format!("\"{}\"", column.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = format!(
        "INSERT INTO \"{}\" ({column_list}) VALUES\n",
        table.replace('"', "\"\"")
    );
    for (index, row) in rows.iter().enumerate() {
        let values = row
            .iter()
            .map(value_to_sql_literal)
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("({values})"));
        out.push_str(if index + 1 < rows.len() { ",\n" } else { ";\n" });
    }
    out
}

/// Exports a query result as CSV/JSON/SQL text (optionally to a local file).
pub fn export_data(
    session_id: &str,
    sql: &str,
    format: &str,
    table: Option<&str>,
    path: Option<&str>,
) -> Result<Value, String> {
    let outcome = query_session(session_id, sql)?;
    let text = match format {
        "csv" => rows_to_csv(&outcome.columns, &outcome.rows),
        "json" => serde_json::to_string_pretty(&serde_json::json!({
            "columns": outcome.columns,
            "rows": outcome.rows,
        }))
        .map_err(|e| format!("JSON 序列化失败：{e}"))?,
        "sql" => {
            let table = table.unwrap_or("exported_table");
            build_inserts(table, &outcome.columns, &outcome.rows)
        }
        other => return Err(format!("不支持的导出格式：{other}")),
    };
    if let Some(path) = path {
        std::fs::write(path, &text).map_err(|e| format!("写入文件失败：{e}"))?;
    }
    Ok(serde_json::json!({
        "format": format,
        "rows": outcome.rows.len(),
        "chars": text.chars().count(),
        "path": path,
        "content": text,
    }))
}

/// Imports CSV/JSON/SQL content into a table for a session.
pub fn import_data(
    session_id: &str,
    table: &str,
    format: &str,
    content: &str,
) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Err("导入内容为空。".to_string());
    }
    let statements: Vec<String> = match format {
        "sql" => split_sql_statements(content),
        "csv" => {
            let mut rows = Vec::new();
            let mut columns = Vec::new();
            for (index, line) in content.lines().enumerate() {
                let fields = split_csv_line(line)?;
                if index == 0 {
                    columns = fields;
                } else {
                    rows.push(fields.into_iter().map(Value::String).collect::<Vec<_>>());
                }
            }
            if columns.is_empty() {
                return Err("CSV 缺少表头。".to_string());
            }
            vec![build_inserts(table, &columns, &rows)]
        }
        "json" => {
            let parsed: Value =
                serde_json::from_str(content).map_err(|e| format!("JSON 解析失败：{e}"))?;
            let (columns, rows) = match parsed {
                Value::Object(map) => {
                    let cols = map
                        .get("columns")
                        .and_then(Value::as_array)
                        .map(|array| {
                            array
                                .iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                        .ok_or_else(|| "JSON 缺少 columns 数组。".to_string())?;
                    let rows = map
                        .get("rows")
                        .and_then(Value::as_array)
                        .map(|array| {
                            array
                                .iter()
                                .filter_map(|row| row.as_array().cloned())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    (cols, rows)
                }
                Value::Array(array) => {
                    let mut cols: Vec<String> = Vec::new();
                    let mut rows = Vec::new();
                    for item in &array {
                        if let Value::Object(obj) = item {
                            for key in obj.keys() {
                                if !cols.contains(key) {
                                    cols.push(key.clone());
                                }
                            }
                        }
                    }
                    for item in array {
                        if let Value::Object(obj) = item {
                            rows.push(
                                cols.iter()
                                    .map(|col| obj.get(col).cloned().unwrap_or(Value::Null))
                                    .collect(),
                            );
                        }
                    }
                    (cols, rows)
                }
                _ => return Err("JSON 结构不识别。".to_string()),
            };
            vec![build_inserts(table, &columns, &rows)]
        }
        other => return Err(format!("不支持的导入格式：{other}")),
    };
    for statement in &statements {
        query_session(session_id, statement)?;
    }
    Ok(serde_json::json!({
        "format": format,
        "statements": statements.len(),
        "imported": true,
    }))
}

/// Splits a SQL text into statements on semicolons outside quotes.
fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in sql.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
            }
            ';' if !in_single && !in_double => {
                if !current.trim().is_empty() {
                    statements.push(current.trim().to_string());
                }
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        statements.push(current.trim().to_string());
    }
    statements
}

/// Returns the per-engine SQL that lists a table's column names.
fn column_sql(engine: &str, table: &str) -> Option<String> {
    let quoted = table.replace('\'', "''");
    match engine {
        "mysql" | "oceanbase" => Some(format!(
            "SELECT column_name FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = '{quoted}'"
        )),
        "postgresql" | "kingbase" | "opengauss" => Some(format!(
            "SELECT column_name FROM information_schema.columns WHERE table_name = '{quoted}'"
        )),
        "sqlite" => Some(format!("SELECT name FROM pragma_table_info('{quoted}')")),
        "duckdb" => Some(format!(
            "SELECT column_name FROM information_schema.columns WHERE table_name = '{quoted}'"
        )),
        "sqlserver" => Some(format!(
            "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_NAME = '{quoted}'"
        )),
        "oracle" | "dm" => Some(format!(
            "SELECT column_name FROM all_tab_columns WHERE table_name = '{quoted}'"
        )),
        "clickhouse" => Some(format!(
            "SELECT name FROM system.columns WHERE table = '{quoted}'"
        )),
        _ => None,
    }
}

/// Lists a session's table names (kind == "table").
fn session_tables(session_id: &str) -> Result<Vec<String>, String> {
    let objects = list_objects(session_id, Some("table"))?;
    Ok(objects
        .into_iter()
        .filter_map(|object| object["name"].as_str().map(str::to_string))
        .collect())
}

/// Lists a table's column names for a session.
fn session_columns(session_id: &str, table: &str) -> Result<Vec<String>, String> {
    let engine = {
        let guard = sessions_map().lock().expect("db sessions lock");
        guard
            .as_ref()
            .and_then(|m| m.get(session_id))
            .map(|s| s.engine.clone())
            .ok_or_else(|| "数据库会话不存在。".to_string())?
    };
    let Some(sql) = column_sql(&engine, table) else {
        return Ok(Vec::new());
    };
    let outcome = query_session(session_id, &sql)?;
    Ok(outcome
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(Value::as_str).map(str::to_string))
        .collect())
}

/// Compares the schema (tables + columns) of two sessions.
pub fn compare_schema(source_session: &str, target_session: &str) -> Result<Value, String> {
    let source_tables = session_tables(source_session)?;
    let target_tables = session_tables(target_session)?;
    let only_in_source: Vec<String> = source_tables
        .iter()
        .filter(|table| !target_tables.contains(table))
        .cloned()
        .collect();
    let only_in_target: Vec<String> = target_tables
        .iter()
        .filter(|table| !source_tables.contains(table))
        .cloned()
        .collect();
    let mut column_diffs = Vec::new();
    for table in &source_tables {
        if !target_tables.contains(table) {
            continue;
        }
        let source_columns = session_columns(source_session, table)?;
        let target_columns = session_columns(target_session, table)?;
        let only_source: Vec<String> = source_columns
            .iter()
            .filter(|column| !target_columns.contains(column))
            .cloned()
            .collect();
        let only_target: Vec<String> = target_columns
            .iter()
            .filter(|column| !source_columns.contains(column))
            .cloned()
            .collect();
        if !only_source.is_empty() || !only_target.is_empty() {
            column_diffs.push(json!({
                "table": table,
                "only_in_source": only_source,
                "only_in_target": only_target,
            }));
        }
    }
    Ok(json!({
        "only_in_source": only_in_source,
        "only_in_target": only_in_target,
        "column_diffs": column_diffs,
    }))
}

/// Compares row counts and distinct counts for common tables of two sessions.
pub fn compare_data(source_session: &str, target_session: &str) -> Result<Value, String> {
    let source_tables = session_tables(source_session)?;
    let target_tables = session_tables(target_session)?;
    let mut diffs = Vec::new();
    for table in &source_tables {
        if !target_tables.contains(table) {
            continue;
        }
        let quoted = table.replace('\'', "''");
        let source_count = query_session(
            source_session,
            &format!("SELECT count(*) AS n FROM \"{quoted}\""),
        )
        .ok()
        .and_then(|outcome| outcome.rows.first().and_then(|row| row.first().cloned()));
        let target_count = query_session(
            target_session,
            &format!("SELECT count(*) AS n FROM \"{quoted}\""),
        )
        .ok()
        .and_then(|outcome| outcome.rows.first().and_then(|row| row.first().cloned()));
        let source_distinct = query_session(
            source_session,
            &format!("SELECT count(*) AS n FROM (SELECT DISTINCT * FROM \"{quoted}\")"),
        )
        .ok()
        .and_then(|outcome| outcome.rows.first().and_then(|row| row.first().cloned()));
        let target_distinct = query_session(
            target_session,
            &format!("SELECT count(*) AS n FROM (SELECT DISTINCT * FROM \"{quoted}\")"),
        )
        .ok()
        .and_then(|outcome| outcome.rows.first().and_then(|row| row.first().cloned()));
        if source_count != target_count || source_distinct != target_distinct {
            diffs.push(json!({
                "table": table,
                "source_rows": source_count,
                "target_rows": target_count,
                "source_distinct": source_distinct,
                "target_distinct": target_distinct,
            }));
        }
    }
    Ok(json!({ "diffs": diffs }))
}

/// Returns the per-engine SQL that lists a table's primary key columns.
fn primary_key_sql(engine: &str, table: &str) -> Option<String> {
    let quoted = table.replace('\'', "''");
    match engine {
        "sqlite" => Some(format!(
            "SELECT name FROM pragma_table_info('{quoted}') WHERE pk > 0"
        )),
        "mysql" | "oceanbase" => Some(format!(
            "SELECT column_name FROM information_schema.key_column_usage WHERE table_schema = DATABASE() AND table_name = '{quoted}' AND constraint_name = 'PRIMARY'"
        )),
        "postgresql" | "kingbase" | "opengauss" | "duckdb" => Some(format!(
            "SELECT kcu.column_name FROM information_schema.table_constraints tc JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_name = '{quoted}'"
        )),
        "sqlserver" => Some(format!(
            "SELECT COL_NAME(ic.object_id, ic.column_id) FROM sys.indexes i JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id WHERE i.is_primary_key = 1 AND OBJECT_NAME(ic.object_id) = '{quoted}'"
        )),
        _ => None,
    }
}

/// Returns the per-engine SQL that lists foreign keys (table, column, ref_table,
/// ref_column).
fn foreign_key_sql(engine: &str) -> Option<&'static str> {
    match engine {
        "mysql" | "oceanbase" => Some(
            "SELECT table_name, column_name, referenced_table_name, referenced_column_name FROM information_schema.key_column_usage WHERE referenced_table_name IS NOT NULL AND table_schema = DATABASE()",
        ),
        "postgresql" | "kingbase" | "opengauss" | "duckdb" => Some(
            "SELECT tc.table_name, kcu.column_name, ccu.table_name AS referenced_table, ccu.column_name AS referenced_column FROM information_schema.table_constraints tc JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema JOIN information_schema.constraint_column_usage ccu ON ccu.constraint_name = tc.constraint_name AND ccu.table_schema = tc.table_schema WHERE tc.constraint_type = 'FOREIGN KEY'",
        ),
        "sqlserver" => Some(
            "SELECT OBJECT_NAME(fkc.parent_object_id) AS table_name, COL_NAME(fkc.parent_object_id, fkc.parent_column_id) AS column_name, OBJECT_NAME(fkc.referenced_object_id) AS referenced_table, COL_NAME(fkc.referenced_object_id, fkc.referenced_column_id) AS referenced_column FROM sys.foreign_key_columns fkc",
        ),
        _ => None,
    }
}

/// Collects ER metadata (tables with columns + primary keys, and foreign-key
/// relationships) for a session, used by the ER diagram renderer.
pub fn er_metadata(session_id: &str) -> Result<Value, String> {
    let engine = {
        let guard = sessions_map().lock().expect("db sessions lock");
        guard
            .as_ref()
            .and_then(|m| m.get(session_id))
            .map(|s| s.engine.clone())
            .ok_or_else(|| "数据库会话不存在。".to_string())?
    };
    let tables = session_tables(session_id)?;
    let mut table_nodes = Vec::new();
    for table in &tables {
        let columns = session_columns(session_id, table)?;
        let primary_key = primary_key_sql(&engine, table)
            .map(|sql| {
                query_session(session_id, &sql)
                    .ok()
                    .map(|outcome| {
                        outcome
                            .rows
                            .iter()
                            .filter_map(|row| {
                                row.first().and_then(Value::as_str).map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        table_nodes.push(json!({
            "name": table,
            "columns": columns,
            "primary_key": primary_key,
        }));
    }
    let relationships = if engine == "sqlite" {
        // SQLite exposes foreign keys per table via PRAGMA foreign_key_list.
        let mut rels = Vec::new();
        for table in &tables {
            let quoted = table.replace('\'', "''");
            if let Ok(outcome) =
                query_session(session_id, &format!("PRAGMA foreign_key_list('{quoted}')"))
            {
                for row in outcome.rows {
                    let column = row.get(3).and_then(Value::as_str).unwrap_or("");
                    let ref_table = row.get(2).and_then(Value::as_str).unwrap_or("");
                    let ref_column = row.get(4).and_then(Value::as_str).unwrap_or("");
                    if !column.is_empty() && !ref_table.is_empty() {
                        rels.push(json!({
                            "table": table,
                            "column": column,
                            "ref_table": ref_table,
                            "ref_column": ref_column,
                        }));
                    }
                }
            }
        }
        rels
    } else {
        match foreign_key_sql(&engine) {
            Some(sql) => query_session(session_id, sql)
                .ok()
                .map(|outcome| {
                    outcome
                        .rows
                        .iter()
                        .filter_map(|row| {
                            let table = row.first().and_then(Value::as_str).unwrap_or("");
                            let column = row.get(1).and_then(Value::as_str).unwrap_or("");
                            let ref_table = row.get(2).and_then(Value::as_str).unwrap_or("");
                            let ref_column = row.get(3).and_then(Value::as_str).unwrap_or("");
                            if table.is_empty() || ref_table.is_empty() {
                                None
                            } else {
                                Some(json!({
                                    "table": table,
                                    "column": column,
                                    "ref_table": ref_table,
                                    "ref_column": ref_column,
                                }))
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    };
    Ok(json!({
        "engine": engine,
        "tables": table_nodes,
        "relationships": relationships,
    }))
}

/// Builds a Redis URL from a parsed profile (redis://[:pass@]host:port/db).
fn redis_url(parsed: &DbProfile) -> String {
    let db = parsed.database.parse::<i64>().unwrap_or(0);
    match &parsed.password {
        Some(password) if !password.is_empty() => {
            if parsed.username.is_empty() {
                format!(
                    "redis://:{}@{}:{}/{}",
                    password, parsed.host, parsed.port, db
                )
            } else {
                format!(
                    "redis://{}:{}@{}:{}/{}",
                    parsed.username, password, parsed.host, parsed.port, db
                )
            }
        }
        _ => format!("redis://{}:{}/{}", parsed.host, parsed.port, db),
    }
}

/// Connects a Redis session (real ConnectionManager handshake).
fn redis_connect(parsed: &DbProfile) -> Result<redis::aio::ConnectionManager, String> {
    let url = redis_url(parsed);
    runtime().block_on(async {
        let client =
            redis::Client::open(url.as_str()).map_err(|e| format!("Redis 配置失败：{e}"))?;
        redis::aio::ConnectionManager::new(client)
            .await
            .map_err(|e| format!("Redis 连接失败：{e}"))
    })
}

/// Converts a redis value into a JSON value.
fn redis_value_to_json(value: redis::Value) -> Value {
    match value {
        redis::Value::Nil => Value::Null,
        redis::Value::Int(i) => json!(i),
        redis::Value::BulkString(bytes) => {
            Value::String(String::from_utf8_lossy(&bytes).to_string())
        }
        redis::Value::SimpleString(s) => Value::String(s),
        redis::Value::Okay => Value::String("OK".to_string()),
        redis::Value::Double(f) => json!(f),
        redis::Value::Boolean(b) => json!(b),
        redis::Value::VerbatimString { text, .. } => Value::String(text),
        redis::Value::Array(items) => {
            Value::Array(items.into_iter().map(redis_value_to_json).collect())
        }
        redis::Value::Set(items) => {
            Value::Array(items.into_iter().map(redis_value_to_json).collect())
        }
        redis::Value::Map(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| {
                    (
                        redis_value_to_json(key).to_string(),
                        redis_value_to_json(value),
                    )
                })
                .collect(),
        ),
        redis::Value::Attribute { data, .. } => redis_value_to_json(*data),
        other => Value::String(format!("{other:?}")),
    }
}

/// Returns a clone of the session's Redis connection handle.
fn redis_session_conn(
    session_id: &str,
) -> Result<std::sync::Arc<tokio::sync::Mutex<redis::aio::ConnectionManager>>, String> {
    let guard = sessions_map().lock().expect("db sessions lock");
    match guard
        .as_ref()
        .and_then(|m| m.get(session_id))
        .and_then(|s| s.connection.as_ref())
    {
        Some(EngineConnection::Redis(conn)) => Ok(std::sync::Arc::clone(conn)),
        Some(_) => Err("会话不是 Redis 连接。".to_string()),
        None => Err("数据库会话不存在或未连接。".to_string()),
    }
}

/// Lists keys matching a SCAN pattern (default "*").
pub fn redis_keys(session_id: &str, pattern: &str) -> Result<Value, String> {
    let conn = redis_session_conn(session_id)?;
    let mut guard = runtime().block_on(async { conn.lock().await });
    let pattern = if pattern.trim().is_empty() {
        "*".to_string()
    } else {
        pattern.to_string()
    };
    runtime().block_on(async {
        let mut cmd = redis::Cmd::new();
        cmd.arg("SCAN")
            .arg(0)
            .arg("MATCH")
            .arg(&pattern)
            .arg("COUNT")
            .arg(1000);
        let (_, keys): (u64, Vec<String>) = cmd
            .query_async(&mut *guard)
            .await
            .map_err(|e| format!("Redis SCAN 失败：{e}"))?;
        Ok::<Value, String>(json!({ "pattern": pattern, "keys": keys }))
    })
}

/// Reads a key's value via GET (raw value as JSON).
pub fn redis_get(session_id: &str, key: &str) -> Result<Value, String> {
    let conn = redis_session_conn(session_id)?;
    let mut guard = runtime().block_on(async { conn.lock().await });
    runtime().block_on(async {
        let value: redis::Value = redis::cmd("GET")
            .arg(key)
            .query_async(&mut *guard)
            .await
            .map_err(|e| format!("Redis GET 失败：{e}"))?;
        Ok::<Value, String>(json!({ "key": key, "value": redis_value_to_json(value) }))
    })
}

/// Writes a key via SET with optional TTL (seconds).
pub fn redis_set(
    session_id: &str,
    key: &str,
    value: &str,
    ttl_seconds: Option<i64>,
) -> Result<Value, String> {
    let conn = redis_session_conn(session_id)?;
    let mut guard = runtime().block_on(async { conn.lock().await });
    runtime().block_on(async {
        let mut cmd = redis::Cmd::new();
        cmd.arg("SET").arg(key).arg(value);
        if let Some(ttl) = ttl_seconds {
            if ttl > 0 {
                cmd.arg("EX").arg(ttl);
            }
        }
        let result: String = cmd
            .query_async(&mut *guard)
            .await
            .map_err(|e| format!("Redis SET 失败：{e}"))?;
        Ok::<Value, String>(json!({
            "key": key,
            "ok": result == "OK",
            "ttl_seconds": ttl_seconds,
        }))
    })
}

/// Reads a key's TTL (seconds; -1 = no expiry, -2 = missing).
pub fn redis_ttl(session_id: &str, key: &str) -> Result<Value, String> {
    let conn = redis_session_conn(session_id)?;
    let mut guard = runtime().block_on(async { conn.lock().await });
    runtime().block_on(async {
        let ttl: i64 = redis::cmd("TTL")
            .arg(key)
            .query_async(&mut *guard)
            .await
            .map_err(|e| format!("Redis TTL 失败：{e}"))?;
        Ok::<Value, String>(json!({ "key": key, "ttl_seconds": ttl }))
    })
}

/// Deletes a key via DEL.
pub fn redis_del(session_id: &str, key: &str) -> Result<Value, String> {
    let conn = redis_session_conn(session_id)?;
    let mut guard = runtime().block_on(async { conn.lock().await });
    runtime().block_on(async {
        let removed: i64 = redis::cmd("DEL")
            .arg(key)
            .query_async(&mut *guard)
            .await
            .map_err(|e| format!("Redis DEL 失败：{e}"))?;
        Ok::<Value, String>(json!({ "key": key, "removed": removed }))
    })
}

/// Reads a key's TYPE.
pub fn redis_type(session_id: &str, key: &str) -> Result<Value, String> {
    let conn = redis_session_conn(session_id)?;
    let mut guard = runtime().block_on(async { conn.lock().await });
    runtime().block_on(async {
        let kind: String = redis::cmd("TYPE")
            .arg(key)
            .query_async(&mut *guard)
            .await
            .map_err(|e| format!("Redis TYPE 失败：{e}"))?;
        Ok::<Value, String>(json!({ "key": key, "type": kind }))
    })
}

/// Runs a Redis console command line (e.g. "GET user:1", "INFO", "SCAN 0").
pub fn redis_console(session_id: &str, command_line: &str) -> Result<Value, String> {
    let conn = redis_session_conn(session_id)?;
    let mut guard = runtime().block_on(async { conn.lock().await });
    let outcome = redis_run(&mut guard, command_line)?;
    Ok(json!({
        "columns": outcome.columns,
        "rows": outcome.rows,
        "affected_rows": outcome.affected_rows,
    }))
}

/// Executes a Redis console command line and returns a query outcome.
fn redis_run(
    con: &mut redis::aio::ConnectionManager,
    command_line: &str,
) -> Result<QueryOutcome, String> {
    let parts: Vec<&str> = command_line.split_whitespace().collect();
    if parts.is_empty() {
        return Err("命令为空。".to_string());
    }
    let value = runtime().block_on(async {
        let mut cmd = redis::Cmd::new();
        cmd.arg(parts[0]);
        for part in &parts[1..] {
            cmd.arg(*part);
        }
        cmd.query_async::<redis::Value>(&mut *con)
            .await
            .map_err(|e| format!("Redis 命令失败：{e}"))
    })?;
    Ok(QueryOutcome {
        columns: vec!["result".to_string()],
        rows: vec![vec![redis_value_to_json(value)]],
        affected_rows: 0,
    })
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
        // T019-T026 wire MySQL/PostgreSQL/SQLite/DuckDB/SQL Server/Oracle/ClickHouse/DM; others stay unwired.
        assert!(engine_available("mysql"));
        assert!(engine_available("postgresql"));
        assert!(engine_available("sqlite"));
        assert!(engine_available("duckdb"));
        assert!(engine_available("sqlserver"));
        assert!(engine_available("oracle"));
        assert!(engine_available("clickhouse"));
        assert!(engine_available("dm"));
        assert!(engine_available("kingbase"));
        assert!(engine_available("gbase"));
        assert!(engine_available("oceanbase"));
        assert!(engine_available("opengauss"));
        assert!(engine_available("iotdb"));
        assert!(DB_ENGINES.iter().all(|(key, _)| {
            *key == "mysql"
                || *key == "postgresql"
                || *key == "sqlite"
                || *key == "duckdb"
                || *key == "sqlserver"
                || *key == "oracle"
                || *key == "clickhouse"
                || *key == "dm"
                || *key == "kingbase"
                || *key == "gbase"
                || *key == "oceanbase"
                || *key == "opengauss"
                || *key == "iotdb"
                || *key == "redis"
                || !engine_available(key)
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
    fn duckdb_roundtrip_works() {
        let dir =
            std::env::temp_dir().join(format!("onehub-duckdb-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "duckdb", "database": dir.to_string_lossy() });
        let create =
            query_inline(&profile, "CREATE TABLE demo (id INTEGER, name VARCHAR)").expect("create");
        assert_eq!(create.affected_rows, 0);
        let insert =
            query_inline(&profile, "INSERT INTO demo VALUES (1, 'onehub')").expect("insert");
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
    fn sqlserver_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "sqlserver", "host": "127.0.0.1", "port": 1, "username": "sa", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");
    }

    #[test]
    fn oracle_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "oracle", "host": "127.0.0.1", "port": 1, "username": "system", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1 FROM dual").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");
    }

    #[test]
    fn clickhouse_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "clickhouse", "host": "127.0.0.1", "port": 1, "username": "default", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");
    }

    #[test]
    fn driver_kind_maps_extensions() {
        assert_eq!(driver_kind("mysql"), "mysql");
        assert_eq!(driver_kind("oceanbase"), "mysql");
        assert_eq!(driver_kind("opengauss"), "postgres");
        assert_eq!(driver_kind("kingbase"), "postgres");
        assert_eq!(driver_kind("dm"), "odbc");
        assert_eq!(driver_kind("gbase"), "odbc");
        assert_eq!(driver_kind("iotdb"), "http");
        let registry = provider_registry();
        assert_eq!(registry.len(), DB_ENGINES.len());
        let dm = registry
            .iter()
            .find(|p| p["engine"] == "dm")
            .expect("dm in registry");
        assert_eq!(dm["driver"], "odbc");
        assert_eq!(dm["available"], serde_json::Value::Bool(true));
    }

    #[test]
    fn dm_odbc_graceful_without_driver() {
        let profile = json!({ "engine": "dm", "host": "127.0.0.1", "port": 5236, "username": "SYSDBA", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("no dm driver");
        assert!(
            connect_err.contains("失败") || connect_err.contains("ODBC"),
            "got {connect_err:?}"
        );
        let query_err = query_inline(&profile, "SELECT 1").expect_err("no dm driver");
        assert!(
            query_err.contains("失败") || query_err.contains("ODBC"),
            "got {query_err:?}"
        );
    }

    #[test]
    fn kingbase_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "kingbase", "host": "127.0.0.1", "port": 1, "username": "system", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");
    }

    #[test]
    fn gbase_odbc_graceful_without_driver() {
        let profile = json!({ "engine": "gbase", "host": "127.0.0.1", "port": 9088, "username": "gbasedbt", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("no gbase driver");
        assert!(
            connect_err.contains("失败") || connect_err.contains("ODBC"),
            "got {connect_err:?}"
        );
        let query_err = query_inline(&profile, "SELECT 1").expect_err("no gbase driver");
        assert!(
            query_err.contains("失败") || query_err.contains("ODBC"),
            "got {query_err:?}"
        );
    }

    #[test]
    fn oceanbase_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "oceanbase", "host": "127.0.0.1", "port": 1, "username": "root", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");
    }

    #[test]
    fn opengauss_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "opengauss", "host": "127.0.0.1", "port": 1, "username": "gaussdb", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SELECT 1").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");
    }

    #[test]
    fn iotdb_refused_endpoints_are_graceful() {
        let profile = json!({ "engine": "iotdb", "host": "127.0.0.1", "port": 1, "username": "root", "password": "x", "connect_timeout_ms": 800 });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(connect_err.contains("失败"), "got {connect_err:?}");

        let query_err = query_inline(&profile, "SHOW VERSION").expect_err("refused");
        assert!(query_err.contains("失败"), "got {query_err:?}");
    }

    #[test]
    fn object_catalog_sql_is_engine_specific() {
        assert!(object_catalog_sql("mysql").contains("information_schema.tables"));
        assert!(object_catalog_sql("postgresql").contains("pg_tables"));
        assert!(object_catalog_sql("sqlite").contains("sqlite_master"));
        assert!(object_catalog_sql("duckdb").contains("information_schema.tables"));
        assert!(object_catalog_sql("oracle").contains("all_tables"));
        assert!(object_catalog_sql("clickhouse").contains("system.tables"));
        assert!(object_catalog_sql("iotdb").contains("SHOW TIMESERIES"));
        assert!(object_catalog_sql("redis").is_empty());
    }

    #[test]
    fn sqlite_object_list_is_real() {
        let dir =
            std::env::temp_dir().join(format!("onehub-sqlite-obj-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "sqlite", "database": dir.to_string_lossy() });
        let session = connect(&profile).expect("connect");
        let _ = query_session(
            &session,
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT)",
        );
        let _ = query_session(&session, "CREATE VIEW demo_view AS SELECT id FROM demo");
        let _ = query_session(&session, "CREATE INDEX demo_idx ON demo(name)");
        let objects = list_objects(&session, None).expect("list");
        let names: Vec<String> = objects
            .iter()
            .filter_map(|o| o["name"].as_str().map(str::to_string))
            .collect();
        assert!(names.contains(&"demo".to_string()), "got {names:?}");
        assert!(names.contains(&"demo_view".to_string()), "got {names:?}");
        assert!(names.contains(&"demo_idx".to_string()), "got {names:?}");
        let tables = list_objects(&session, Some("table")).expect("filtered");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0]["name"], "demo");
        let _ = close_session(&session);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn duckdb_object_list_is_real() {
        let dir = std::env::temp_dir().join(format!("onehub-duckdb-obj-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "duckdb", "database": dir.to_string_lossy() });
        let session = connect(&profile).expect("connect");
        let _ = query_session(&session, "CREATE TABLE demo (id INTEGER, name VARCHAR)");
        let objects = list_objects(&session, None).expect("list");
        let names: Vec<String> = objects
            .iter()
            .filter_map(|o| o["name"].as_str().map(str::to_string))
            .collect();
        assert!(names.contains(&"demo".to_string()), "got {names:?}");
        let _ = close_session(&session);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn explain_works_on_sqlite_and_rejects_unsupported() {
        let dir = std::env::temp_dir().join(format!(
            "onehub-sqlite-explain-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "sqlite", "database": dir.to_string_lossy() });
        let session = connect(&profile).expect("connect");
        let _ = query_session(
            &session,
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT)",
        );
        let plan = explain(&session, "SELECT * FROM demo WHERE id = 1").expect("explain");
        assert!(!plan.columns.is_empty(), "plan columns");
        let _ = close_session(&session);
        let _ = std::fs::remove_file(&dir);

        let unsupported = explain("db-missing", "SELECT 1").expect_err("no session");
        assert!(unsupported.contains("会话不存在"));
        assert!(engine_supports_limit("mysql"));
        assert!(engine_supports_limit("sqlite"));
        assert!(!engine_supports_limit("sqlserver"));
        assert!(!engine_supports_limit("oracle"));
    }

    #[test]
    fn object_list_refused_session_is_graceful() {
        let profile = json!({ "engine": "postgresql", "host": "127.0.0.1", "port": 1, "username": "u", "password": "x", "connect_timeout_ms": 800 });
        let session = connect(&profile);
        if let Ok(session_id) = session {
            let err = list_objects(&session_id, None).expect_err("refused");
            assert!(err.contains("失败"), "got {err:?}");
            let _ = close_session(&session_id);
        }
    }

    #[test]
    fn csv_export_import_roundtrip_on_sqlite() {
        let dir =
            std::env::temp_dir().join(format!("onehub-sqlite-xfer-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "sqlite", "database": dir.to_string_lossy() });
        let session = connect(&profile).expect("connect");
        let _ = query_session(
            &session,
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT, note TEXT)",
        );
        let _ = query_session(
            &session,
            "INSERT INTO demo(name, note) VALUES ('alice', 'a, \"quoted\"')",
        );
        let exported =
            export_data(&session, "SELECT * FROM demo", "csv", None, None).expect("export csv");
        assert_eq!(exported["rows"], json!(1));
        let csv = exported["content"].as_str().expect("csv content");
        assert!(csv.contains("alice"), "csv={csv}");
        assert!(csv.contains("\"a, \"\"quoted\"\"\""), "csv quoted field");
        let json_out =
            export_data(&session, "SELECT * FROM demo", "json", None, None).expect("export json");
        assert!(json_out["content"]
            .as_str()
            .expect("json")
            .contains("alice"));
        let sql_out = export_data(&session, "SELECT * FROM demo", "sql", Some("demo"), None)
            .expect("export sql");
        assert!(sql_out["content"]
            .as_str()
            .expect("sql")
            .contains("INSERT INTO"));
        let _ = query_session(&session, "CREATE TABLE import_demo (name TEXT, note TEXT)");
        let imported = import_data(
            &session,
            "import_demo",
            "csv",
            "name,note\nbob,hello world\ncarol,\"x, y\"\n",
        )
        .expect("import csv");
        assert_eq!(imported["imported"], true);
        let check =
            query_session(&session, "SELECT count(*) AS n FROM import_demo").expect("count");
        assert_eq!(check.rows[0][0], json!(2));
        let sql_import = import_data(
            &session,
            "import_demo",
            "sql",
            "INSERT INTO import_demo VALUES ('dave', 'z');",
        )
        .expect("import sql");
        assert_eq!(sql_import["statements"], json!(1));
        let json_import = import_data(
            &session,
            "import_demo",
            "json",
            "[{\"name\":\"erin\",\"note\":\"n1\"},{\"name\":\"frank\",\"note\":\"n2\"}]",
        )
        .expect("import json");
        assert_eq!(json_import["imported"], true);
        let _ = close_session(&session);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn export_rejects_unknown_format() {
        let dir = std::env::temp_dir().join(format!(
            "onehub-sqlite-xferbad-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "sqlite", "database": dir.to_string_lossy() });
        let session = connect(&profile).expect("connect");
        let _ = query_session(&session, "CREATE TABLE t (id INTEGER)");
        let err = export_data(&session, "SELECT * FROM t", "xml", None, None).expect_err("xml");
        assert!(err.contains("不支持的导出格式"));
        let _ = close_session(&session);
        let _ = std::fs::remove_file(&dir);
    }
    #[test]
    fn schema_data_compare_on_sqlite() {
        let dir_a =
            std::env::temp_dir().join(format!("onehub-cmp-a-{}.sqlite", std::process::id()));
        let dir_b =
            std::env::temp_dir().join(format!("onehub-cmp-b-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&dir_a);
        let _ = std::fs::remove_file(&dir_b);
        let profile_a = json!({ "engine": "sqlite", "database": dir_a.to_string_lossy() });
        let profile_b = json!({ "engine": "sqlite", "database": dir_b.to_string_lossy() });
        let session_a = connect(&profile_a).expect("connect a");
        let session_b = connect(&profile_b).expect("connect b");
        let _ = query_session(
            &session_a,
            "CREATE TABLE common (id INTEGER PRIMARY KEY, name TEXT)",
        );
        let _ = query_session(&session_a, "CREATE TABLE only_a (x INTEGER)");
        let _ = query_session(
            &session_a,
            "INSERT INTO common(name) VALUES ('alice'), ('bob')",
        );
        let _ = query_session(
            &session_b,
            "CREATE TABLE common (id INTEGER PRIMARY KEY, name TEXT, extra TEXT)",
        );
        let _ = query_session(
            &session_b,
            "INSERT INTO common(name, extra) VALUES ('alice', 'e1')",
        );
        let schema = compare_schema(&session_a, &session_b).expect("compare schema");
        assert_eq!(schema["only_in_source"], json!(["only_a"]));
        assert!(schema["only_in_target"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false));
        let common_diff = schema["column_diffs"]
            .as_array()
            .and_then(|diffs| diffs.iter().find(|d| d["table"] == "common"));
        assert!(
            common_diff.is_some(),
            "column diff for common expected: {schema}"
        );
        let data = compare_data(&session_a, &session_b).expect("compare data");
        let common_data = data["diffs"]
            .as_array()
            .and_then(|diffs| diffs.iter().find(|d| d["table"] == "common"));
        assert!(
            common_data.is_some(),
            "data diff for common expected: {data}"
        );
        let _ = close_session(&session_a);
        let _ = close_session(&session_b);
        let _ = std::fs::remove_file(&dir_a);
        let _ = std::fs::remove_file(&dir_b);
    }

    #[test]
    fn compare_missing_session_is_graceful() {
        let err = compare_schema("db-missing", "db-missing").expect_err("no session");
        assert!(err.contains("会话不存在"));
    }
    #[test]
    fn er_metadata_works_on_sqlite() {
        let dir =
            std::env::temp_dir().join(format!("onehub-er-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let profile = json!({ "engine": "sqlite", "database": dir.to_string_lossy() });
        let session = connect(&profile).expect("connect");
        for sql in [
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, note TEXT, FOREIGN KEY (user_id) REFERENCES users(id))",
        ] {
            query_session(&session, sql).expect("setup sql");
        }
        let meta = er_metadata(&session).expect("er metadata");
        assert_eq!(meta["engine"], "sqlite");
        let tables = meta["tables"].as_array().expect("tables array");
        assert!(tables.len() >= 2, "expected users+orders, got {tables:?}");
        let users = tables
            .iter()
            .find(|t| t["name"] == "users")
            .expect("users node");
        assert_eq!(users["primary_key"][0], "id", "users pk: {users:?}");
        let orders = tables
            .iter()
            .find(|t| t["name"] == "orders")
            .expect("orders node");
        let order_columns = orders["columns"].as_array().expect("orders columns");
        assert!(order_columns.iter().any(|c| c == "user_id"));
        let rels = meta["relationships"].as_array().expect("relationships");
        assert!(
            rels.iter().any(|r| {
                r["table"] == "orders"
                    && r["column"] == "user_id"
                    && r["ref_table"] == "users"
                    && r["ref_column"] == "id"
            }),
            "orders->users FK not found: {rels:?}"
        );
        assert!(close_session(&session));
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn er_metadata_missing_session_is_graceful() {
        let err = er_metadata("no-such-session").expect_err("missing session");
        assert!(err.contains("会话不存在"), "got {err:?}");
    }

    #[test]
    fn proxy_route_parsing_and_resolution() {
        let direct = DbProfile::parse(&json!({ "engine": "mysql", "host": "db", "port": 3306 }))
            .expect("direct");
        let route = db_proxy_route(&direct).expect("direct route");
        assert_eq!(route["proxy_type"], "none");
        assert_eq!(route["via"], "direct");
        assert_eq!(route["endpoint"]["host"], "db");

        let socks = DbProfile::parse(&json!({
            "engine": "postgresql",
            "host": "db",
            "port": 5432,
            "proxy_type": "socks5",
            "proxy_host": "127.0.0.1",
            "proxy_port": 1080,
        }))
        .expect("socks profile");
        let socks_route = db_proxy_route(&socks).expect("socks route");
        assert_eq!(socks_route["via"], "socks5");
        assert_eq!(socks_route["endpoint"]["port"], 1080);

        let http = DbProfile::parse(&json!({
            "engine": "mysql",
            "host": "db",
            "port": 3306,
            "proxy_type": "http",
            "proxy_host": "proxy.local",
            "proxy_port": 3128,
        }))
        .expect("http profile");
        assert_eq!(db_proxy_route(&http).expect("http route")["via"], "http");

        let incomplete = DbProfile::parse(&json!({
            "engine": "mysql",
            "host": "db",
            "proxy_type": "socks5",
            "proxy_host": "",
        }))
        .expect("incomplete profile");
        let err = db_proxy_route(&incomplete).expect_err("incomplete");
        assert!(err.contains("不完整"), "got {err:?}");

        let unknown = DbProfile::parse(&json!({
            "engine": "mysql",
            "host": "db",
            "proxy_type": "weird",
        }))
        .expect("unknown proxy profile");
        let err = db_proxy_route(&unknown).expect_err("unknown");
        assert!(err.contains("未知代理类型"), "got {err:?}");
    }

    #[test]
    fn proxy_route_ssh_requires_running_tunnel() {
        let profile = DbProfile::parse(&json!({
            "engine": "mysql",
            "host": "db",
            "port": 3306,
            "proxy_type": "ssh",
            "tunnel_rule_id": "tunnel-missing",
        }))
        .expect("ssh profile");
        let err = db_proxy_route(&profile).expect_err("tunnel not running");
        assert!(err.contains("未运行"), "got {err:?}");

        crate::tunnels::seed_runtime_state("tunnel-db", "127.0.0.1", 23306);
        let running = DbProfile::parse(&json!({
            "engine": "mysql",
            "host": "db",
            "port": 3306,
            "proxy_type": "ssh",
            "tunnel_rule_id": "tunnel-db",
        }))
        .expect("ssh running profile");
        let route = db_proxy_route(&running).expect("running route");
        assert_eq!(route["via"], "ssh-tunnel");
        assert_eq!(route["endpoint"]["host"], "127.0.0.1");
        assert_eq!(route["endpoint"]["port"], 23306);
    }

    #[test]
    fn test_connection_reports_route() {
        let result = test_connection(&json!({
            "engine": "postgresql",
            "host": "127.0.0.1",
            "port": 1,
            "proxy_type": "socks5",
            "proxy_host": "127.0.0.1",
            "proxy_port": 1,
            "connect_timeout_ms": 500,
        }));
        assert_eq!(result["ok"], true);
        assert_eq!(result["reachable"], false);
        assert_eq!(result["route"]["proxy_type"], "socks5");
        assert!(result["message"].as_str().unwrap_or("").contains("代理"));

        let direct = test_connection(&json!({
            "engine": "postgresql",
            "host": "127.0.0.1",
            "port": 1,
            "connect_timeout_ms": 500,
        }));
        assert_eq!(direct["route"]["via"], "direct");
    }

    #[tokio::test]
    async fn proxy_connect_socks5_loopback() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut head = [0u8; 2];
            stream.read_exact(&mut head).await.expect("greeting");
            let mut methods = vec![0u8; head[1] as usize];
            stream.read_exact(&mut methods).await.expect("methods");
            stream.write_all(&[0x05, 0x00]).await.expect("select");
            let mut request = [0u8; 4];
            stream.read_exact(&mut request).await.expect("request head");
            if request[3] == 0x03 {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await.expect("len");
                let mut domain = vec![0u8; len[0] as usize];
                stream.read_exact(&mut domain).await.expect("domain");
            } else {
                let mut ip = [0u8; 4];
                stream.read_exact(&mut ip).await.expect("ip");
            }
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).await.expect("port");
            let reply = [0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x0d, 0x9c];
            stream.write_all(&reply).await.expect("reply");
        });
        let config = ProxyConfig {
            proxy_type: "socks5".to_string(),
            proxy_host: addr.ip().to_string(),
            proxy_port: addr.port(),
            ..ProxyConfig::default()
        };
        let result = proxy_connect(&config, "db.internal", 3306, 2000).await;
        assert!(result.is_ok(), "got {result:?}");
        server.await.expect("server joined");
    }

    #[test]
    fn redis_refused_endpoints_are_graceful() {
        let profile = json!({
            "engine": "redis",
            "host": "127.0.0.1",
            "port": 1,
            "password": "x",
            "connect_timeout_ms": 500,
        });
        let connect_err = connect(&profile).expect_err("refused");
        assert!(
            connect_err.contains("失败") || connect_err.contains("拒绝"),
            "got {connect_err:?}"
        );
        assert!(redis_console("no-such-session", "PING").is_err());
    }

    #[test]
    fn redis_missing_session_is_graceful() {
        for (label, result) in [
            ("keys", redis_keys("no-session", "*")),
            ("get", redis_get("no-session", "k")),
            ("set", redis_set("no-session", "k", "v", None)),
            ("ttl", redis_ttl("no-session", "k")),
            ("del", redis_del("no-session", "k")),
            ("type", redis_type("no-session", "k")),
            ("console", redis_console("no-session", "PING")),
        ] {
            assert!(result.is_err(), "{label} should fail for missing session");
        }
    }

    #[test]
    fn redis_value_to_json_converts_shapes() {
        assert_eq!(redis_value_to_json(redis::Value::Nil), Value::Null);
        assert_eq!(redis_value_to_json(redis::Value::Int(42)), json!(42));
        assert_eq!(
            redis_value_to_json(redis::Value::BulkString(b"hi".to_vec())),
            Value::String("hi".to_string())
        );
        assert_eq!(
            redis_value_to_json(redis::Value::SimpleString("PONG".to_string())),
            Value::String("PONG".to_string())
        );
        assert_eq!(
            redis_value_to_json(redis::Value::Okay),
            Value::String("OK".to_string())
        );
        assert_eq!(
            redis_value_to_json(redis::Value::Array(vec![
                redis::Value::Int(1),
                redis::Value::Int(2)
            ])),
            json!([1, 2])
        );
    }

    #[test]
    fn unwired_engine_connect_is_honest() {
        let err = connect(
            &json!({ "engine": "mongodb", "host": "127.0.0.1", "username": "root", "password": "x" }),
        )
        .expect_err("mongodb not wired");
        assert!(err.contains("未接入"), "got {err:?}");
        assert!(!close_session("db-missing"));
    }
}
