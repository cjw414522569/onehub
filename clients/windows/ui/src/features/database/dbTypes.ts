export type DbEngineKey =
  | "mysql"
  | "postgresql"
  | "sqlite"
  | "duckdb"
  | "sqlserver"
  | "oracle"
  | "clickhouse"
  | "dm"
  | "kingbase"
  | "gbase"
  | "oceanbase"
  | "opengauss"
  | "iotdb"
  | "redis"
  | "mongodb";

export interface DbEngineInfo {
  engine: DbEngineKey;
  label: string;
  available: boolean;
}

export interface DbConnectionInput {
  engine: DbEngineKey;
  name?: string;
  host?: string;
  port?: number;
  username?: string;
  password?: string;
  database?: string;
  ssl?: boolean;
  connect_timeout_ms?: number;
}

export interface DbConnectionProfile extends DbConnectionInput {
  id: string;
  protocol?: string;
  created_at?: string;
  updated_at?: string;
}

export interface DbTestResult {
  ok: boolean;
  reachable: boolean;
  latency_ms: number | null;
  engine: string;
  engine_available: boolean;
  message: string;
}

export interface DbConnectResult {
  session_id: string;
}

export interface DbQueryRequest {
  session_id?: string;
  sql: string;
  engine?: DbEngineKey;
  host?: string;
  port?: number;
  username?: string;
  password?: string;
  database?: string;
}

export interface DbQueryResult {
  columns: string[];
  rows: unknown[][];
  affected_rows: number;
}
