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
  proxy_type?: string;
  proxy_host?: string;
  proxy_port?: number;
  proxy_username?: string;
  proxy_password?: string;
  tunnel_rule_id?: string;
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
  route?: DbProxyRoute | null;
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

export interface DbObjectInfo {
  name: string;
  kind: string;
}

export interface DbCompareSchema {
  only_in_source?: string[];
  only_in_target?: string[];
  column_diffs?: unknown[];
}

export interface DbCompareData {
  diffs?: unknown[];
}

export interface DbCompareResult {
  schema?: DbCompareSchema | null;
  data?: DbCompareData | null;
}

export interface DbQueryResult {
  columns: string[];
  rows: unknown[][];
  affected_rows: number;
}
export interface DbErTable {
  name: string;
  columns: string[];
  primary_key: string[];
}

export interface DbErRelationship {
  table: string;
  column: string;
  ref_table: string;
  ref_column: string;
}

export interface DbErMetadata {
  engine: string;
  tables: DbErTable[];
  relationships: DbErRelationship[];
}

export interface DbProxyRoute {
  proxy_type: string;
  direct?: { host: string; port: number } | null;
  endpoint?: { host: string; port: number } | null;
  via?: string;
  tunnel_rule_id?: string;
  note?: string;
}

export interface RedisKeyList {
  pattern: string;
  keys: string[];
}

export interface RedisKeyValue {
  key: string;
  value: unknown;
}

export interface RedisSetResult {
  key: string;
  ok: boolean;
  ttl_seconds?: number | null;
}

export interface RedisTtlResult {
  key: string;
  ttl_seconds: number;
}

export interface RedisDelResult {
  key: string;
  removed: number;
}

export interface RedisTypeResult {
  key: string;
  type: string;
}

export interface RedisConsoleResult {
  columns: string[];
  rows: unknown[][];
  affected_rows: number;
}

export interface MongoCollectionsResult {
  database: string;
  collections: string[];
}

export interface MongoDocumentsResult {
  count: number;
  documents: unknown[];
}

export interface MongoInsertResult {
  inserted_id: unknown;
}

export interface MongoUpdateResult {
  matched: number;
  modified: number;
}

export interface MongoDeleteResult {
  deleted: number;
}
