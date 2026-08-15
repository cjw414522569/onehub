import { useCallback, useEffect, useState } from "react";
import { SqlEditor } from "./SqlEditor";
import {
  dbConnectionConnect,
  dbConnectionDelete,
  dbConnectionList,
  dbConnectionSave,
  dbConnectionTest,
  dbEngineList,
  dbExplain,
  dbObjectList,
  dbQuery,
} from "../../shared/tauri/commands";
import type {
  DbConnectionInput,
  DbConnectionProfile,
  DbEngineInfo,
  DbObjectInfo,
  DbQueryResult,
  DbTestResult,
} from "./dbTypes";

interface DatabasePanelProps {
  open: boolean;
  onClose: () => void;
}

const DEFAULT_ENGINE = "mysql";

function defaultPort(engine: string): number {
  switch (engine) {
    case "mysql":
    case "oceanbase":
      return 3306;
    case "postgresql":
    case "kingbase":
    case "opengauss":
    case "gbase":
      return 5432;
    case "sqlserver":
    case "dm":
      return 1433;
    case "oracle":
      return 1521;
    case "clickhouse":
      return 8123;
    case "iotdb":
      return 6667;
    case "redis":
      return 6379;
    case "mongodb":
      return 27017;
    default:
      return 0;
  }
}

function emptyForm(): DbConnectionInput {
  return {
    engine: DEFAULT_ENGINE,
    name: "",
    host: "127.0.0.1",
    port: 3306,
    username: "root",
    password: "",
    database: "",
    ssl: false,
  };
}

function isFileEngine(engine: string): boolean {
  return engine === "sqlite" || engine === "duckdb";
}

export function DatabasePanel({ open, onClose }: DatabasePanelProps) {
  const [connections, setConnections] = useState<DbConnectionProfile[]>([]);
  const [engines, setEngines] = useState<DbEngineInfo[]>([]);
  const [form, setForm] = useState<DbConnectionInput>(emptyForm());
  const [editingId, setEditingId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [sqlTabs, setSqlTabs] = useState<{ id: string; title: string; sql: string }[]>([
    { id: "sql-1", title: "SQL 1", sql: "" },
  ]);
  const [activeSqlTabId, setActiveSqlTabId] = useState("sql-1");
  const activeSqlTab = sqlTabs.find((tab) => tab.id === activeSqlTabId) || sqlTabs[0];
  const [queryResult, setQueryResult] = useState<DbQueryResult | null>(null);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(100);
  const [planResult, setPlanResult] = useState<DbQueryResult | null>(null);
  const [objects, setObjects] = useState<DbObjectInfo[]>([]);
  const [objectsLoaded, setObjectsLoaded] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [conns, engs] = await Promise.all([
        dbConnectionList().catch(() => [] as DbConnectionProfile[]),
        dbEngineList().catch(() => [] as DbEngineInfo[]),
      ]);
      setConnections(conns);
      setEngines(engs);
    } catch {
      // Bridge unavailable (e.g. browser preview); keep empty lists.
    }
  }, []);

  useEffect(() => {
    if (open) {
      void refresh();
      setMessage(null);
    }
  }, [open, refresh]);

  if (!open) {
    return null;
  }

  const onEngineChange = (engine: string) => {
    setForm((prev) => ({
      ...prev,
      engine: engine as DbConnectionInput["engine"],
      port: defaultPort(engine),
    }));
  };

  const save = async () => {
    setBusy(true);
    setMessage(null);
    try {
      await dbConnectionSave(form);
      setForm(emptyForm());
      setEditingId(null);
      await refresh();
      setMessage("已保存数据库连接。");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const testNow = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result: DbTestResult = await dbConnectionTest(form);
      setMessage(`${result.message}（引擎已接入：${result.engine_available ? "是" : "否"}）`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const connect = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await dbConnectionConnect(form);
      setSessionId(result.session_id);
      setMessage(`连接会话已建立：${result.session_id}`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const limitSupported = ["mysql", "oceanbase", "postgresql", "kingbase", "opengauss", "sqlite", "duckdb", "clickhouse"].includes(form.engine);

  const paginatedSql = (sqlText: string, pageNumber: number) => {
    if (!limitSupported) {
      return sqlText;
    }
    return `${sqlText.trim()} LIMIT ${pageSize} OFFSET ${(pageNumber - 1) * pageSize}`;
  };

  const runQuery = async (sqlText: string, pageNumber = 1) => {
    setBusy(true);
    setMessage(null);
    try {
      const result: DbQueryResult = await dbQuery({
        session_id: sessionId || undefined,
        sql: paginatedSql(sqlText, pageNumber),
        engine: form.engine,
        host: form.host,
        port: form.port,
        username: form.username,
        password: form.password,
        database: form.database,
      });
      setQueryResult(result);
      setPage(pageNumber);
      setPlanResult(null);
      setMessage(
        result.affected_rows > 0
          ? `执行完成，影响行数：${result.affected_rows}`
          : `查询完成，返回 ${result.rows.length} 行（第 ${pageNumber} 页）`,
      );
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const runExplain = async () => {
    if (!sessionId) {
      setMessage("请先连接数据库。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result = await dbExplain(sessionId, activeSqlTab.sql);
      setPlanResult(result);
      setMessage(`执行计划已返回 ${result.rows.length} 行。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const addSqlTab = () => {
    const id = `sql-${sqlTabs.length + 1}-${Date.now()}`;
    setSqlTabs((tabs) => [...tabs, { id, title: `SQL ${tabs.length + 1}`, sql: "" }]);
    setActiveSqlTabId(id);
  };

  const closeSqlTab = (id: string) => {
    setSqlTabs((tabs) => {
      const next = tabs.filter((tab) => tab.id !== id);
      if (next.length === 0) {
        const freshId = "sql-empty";
        setActiveSqlTabId(freshId);
        return [{ id: freshId, title: "SQL 1", sql: "" }];
      }
      if (id === activeSqlTabId) {
        setActiveSqlTabId(next[0].id);
      }
      return next;
    });
  };

  const updateSqlTab = (id: string, nextSql: string) => {
    setSqlTabs((tabs) => tabs.map((tab) => (tab.id === id ? { ...tab, sql: nextSql } : tab)));
  };

  const loadObjects = async () => {
    if (!sessionId) {
      setMessage("请先连接数据库。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result = await dbObjectList(sessionId);
      setObjects(result);
      setObjectsLoaded(true);
      setMessage(`已加载 ${result.length} 个对象。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: string) => {
    setBusy(true);
    try {
      await dbConnectionDelete(id);
      await refresh();
      if (editingId === id) {
        setEditingId(null);
        setForm(emptyForm());
      }
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const edit = (conn: DbConnectionProfile) => {
    const engine = conn.engine || conn.protocol || DEFAULT_ENGINE;
    setEditingId(conn.id);
    setForm({
      engine: engine as DbConnectionInput["engine"],
      name: conn.name || "",
      host: conn.host || "127.0.0.1",
      port: conn.port || defaultPort(engine),
      username: conn.username || "",
      password: conn.password || "",
      database: conn.database || "",
      ssl: Boolean(conn.ssl),
    });
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="数据库工作台"
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 2000,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.35)",
      }}
    >
      <div
        style={{
          width: 760,
          maxWidth: "94vw",
          maxHeight: "88vh",
          overflow: "auto",
          background: "#f5f6f8",
          color: "#1f2328",
          borderRadius: 8,
          padding: 16,
          boxShadow: "0 8px 32px rgba(0,0,0,0.25)",
          fontFamily: "system-ui, sans-serif",
          fontSize: 13,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
          <h2 style={{ margin: 0, fontSize: 16 }}>数据库工作台</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            style={{ border: "none", background: "transparent", fontSize: 20, cursor: "pointer", lineHeight: 1 }}
          >
            ×
          </button>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
          <section>
            <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>连接列表</h3>
            {connections.length === 0 ? (
              <p style={{ margin: 0, color: "#6b7280" }}>暂无数据库连接，请先在右侧新建。</p>
            ) : (
              <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                {connections.map((conn) => (
                  <li
                    key={conn.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      padding: "6px 8px",
                      borderBottom: "1px solid #e5e7eb",
                    }}
                  >
                    <span>{conn.name || conn.host || conn.engine}</span>
                    <span style={{ display: "flex", gap: 6 }}>
                      <button type="button" onClick={() => edit(conn)}>
                        编辑
                      </button>
                      <button type="button" onClick={() => void remove(conn.id)}>
                        删除
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section>
            <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>{editingId ? "编辑连接" : "新建连接"}</h3>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
              <label style={{ display: "grid", gap: 2 }}>
                引擎
                <select value={form.engine} onChange={(event) => onEngineChange(event.target.value)}>
                  {engines.map((engine) => (
                    <option key={engine.engine} value={engine.engine}>
                      {engine.label}
                      {engine.available ? "" : "（待接入）"}
                    </option>
                  ))}
                </select>
              </label>
              <label style={{ display: "grid", gap: 2 }}>
                名称
                <input
                  value={form.name || ""}
                  onChange={(event) => setForm({ ...form, name: event.target.value })}
                  placeholder="可选"
                />
              </label>
              <label style={{ display: "grid", gap: 2 }}>
                主机
                <input
                  value={form.host || ""}
                  onChange={(event) => setForm({ ...form, host: event.target.value })}
                  disabled={isFileEngine(form.engine)}
                />
              </label>
              <label style={{ display: "grid", gap: 2 }}>
                端口
                <input
                  type="number"
                  value={form.port ?? 0}
                  onChange={(event) => setForm({ ...form, port: Number(event.target.value) })}
                  disabled={isFileEngine(form.engine)}
                />
              </label>
              <label style={{ display: "grid", gap: 2 }}>
                用户名
                <input
                  value={form.username || ""}
                  onChange={(event) => setForm({ ...form, username: event.target.value })}
                />
              </label>
              <label style={{ display: "grid", gap: 2 }}>
                密码
                <input
                  type="password"
                  value={form.password || ""}
                  onChange={(event) => setForm({ ...form, password: event.target.value })}
                />
              </label>
              <label style={{ display: "grid", gap: 2, gridColumn: "1 / -1" }}>
                数据库 / 文件路径
                <input
                  value={form.database || ""}
                  onChange={(event) => setForm({ ...form, database: event.target.value })}
                  placeholder={isFileEngine(form.engine) ? "本地文件路径" : "数据库名（可选）"}
                />
              </label>
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
              <button type="button" onClick={() => void testNow()} disabled={busy}>
                测试连接
              </button>
              <button type="button" onClick={() => void connect()} disabled={busy}>
                连接
              </button>
              <button type="button" onClick={() => void save()} disabled={busy}>
                {editingId ? "保存修改" : "保存"}
              </button>
            </div>
          </section>
        </div>

        <section style={{ marginTop: 14 }}>
          <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>对象浏览器{sessionId ? "（已连接）" : ""}</h3>
          <button type="button" onClick={() => void loadObjects()} disabled={busy || !sessionId}>
            加载对象
          </button>
          {objectsLoaded ? (
            <div style={{ marginTop: 8, maxHeight: 160, overflow: "auto", border: "1px solid #d1d5db", borderRadius: 4, padding: 6 }}>
              {objects.length === 0 ? (
                <p style={{ margin: 0, color: "#6b7280" }}>未发现对象。</p>
              ) : (
                ["table", "view", "index", "procedure"].map((kind) => {
                  const group = objects.filter((obj) => obj.kind === kind || (kind === "table" && obj.kind === "BASE TABLE"));
                  if (group.length === 0) {
                    return null;
                  }
                  return (
                    <div key={kind} style={{ marginBottom: 4 }}>
                      <strong style={{ fontSize: 12 }}>{kind.toUpperCase()}（{group.length}）</strong>
                      <ul style={{ listStyle: "none", margin: "2px 0 0", paddingLeft: 12, fontSize: 12 }}>
                        {group.map((obj) => (
                          <li key={obj.name}>{obj.name}</li>
                        ))}
                      </ul>
                    </div>
                  );
                })
              )}
            </div>
          ) : null}
        </section>

        <section style={{ marginTop: 14 }}>
          <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>SQL 编辑器{sessionId ? "（已连接）" : "（未连接，使用表单内联连接）"}（Ctrl+Enter 执行）</h3>
          <div style={{ display: "flex", alignItems: "center", gap: 4, marginBottom: 4, flexWrap: "wrap" }}>
            {sqlTabs.map((tab) => (
              <span key={tab.id} style={{ display: "inline-flex", alignItems: "center", gap: 4, border: tab.id === activeSqlTabId ? "1px solid #2374c6" : "1px solid #d1d5db", borderRadius: 4, padding: "2px 6px", fontSize: 12, background: tab.id === activeSqlTabId ? "#e8f0fe" : "transparent" }}>
                <button type="button" onClick={() => setActiveSqlTabId(tab.id)} style={{ border: "none", background: "transparent", cursor: "pointer", padding: 0, fontSize: 12 }}>
                  {tab.title}
                </button>
                <button type="button" onClick={() => closeSqlTab(tab.id)} aria-label="关闭标签" style={{ border: "none", background: "transparent", cursor: "pointer", padding: 0, fontSize: 12 }}>
                  ×
                </button>
              </span>
            ))}
            <button type="button" onClick={addSqlTab} style={{ border: "none", background: "transparent", cursor: "pointer", fontSize: 14 }}>
              +
            </button>
          </div>
          <SqlEditor
            value={activeSqlTab.sql}
            onChange={(nextSql) => updateSqlTab(activeSqlTab.id, nextSql)}
            onRun={() => void runQuery(activeSqlTab.sql)}
            objectNames={objects.map((obj) => obj.name)}
          />
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 6 }}>
            <button type="button" onClick={() => void runQuery(activeSqlTab.sql)} disabled={busy || !activeSqlTab.sql.trim()}>
              执行
            </button>
            <button type="button" onClick={() => void runExplain()} disabled={busy || !sessionId || !activeSqlTab.sql.trim()}>
              执行计划
            </button>
            {limitSupported ? (
              <>
                <button type="button" onClick={() => void runQuery(activeSqlTab.sql, Math.max(1, page - 1))} disabled={busy || page <= 1}>
                  上一页
                </button>
                <span style={{ fontSize: 12 }}>第 {page} 页</span>
                <button type="button" onClick={() => void runQuery(activeSqlTab.sql, page + 1)} disabled={busy || (queryResult ? queryResult.rows.length < pageSize : true)}>
                  下一页
                </button>
                <select value={pageSize} onChange={(event) => setPageSize(Number(event.target.value))} style={{ fontSize: 12 }}>
                  {[50, 100, 200, 500].map((size) => (
                    <option key={size} value={size}>
                      {size} 行/页
                    </option>
                  ))}
                </select>
              </>
            ) : (
              <span style={{ fontSize: 12, color: "#6b7280" }}>该引擎不支持 LIMIT 分页</span>
            )}
          </div>
          {(queryResult || planResult) ? (
            <div style={{ marginTop: 8, overflow: "auto", maxHeight: 240 }}>
              <p style={{ margin: "0 0 4px", fontSize: 12, color: "#6b7280" }}>
                {planResult ? "执行计划" : "查询结果"}：{planResult || queryResult ? (planResult || queryResult)?.rows.length : 0} 行
              </p>
              <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 12 }}>
                <thead>
                  <tr>
                    {(planResult || queryResult)?.columns.map((column) => (
                      <th key={column} style={{ border: "1px solid #d1d5db", padding: "4px 8px", textAlign: "left" }}>
                        {column}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {(planResult || queryResult)?.rows.map((row, rowIndex) => (
                    <tr key={rowIndex}>
                      {row.map((cell, cellIndex) => (
                        <td key={cellIndex} style={{ border: "1px solid #d1d5db", padding: "4px 8px" }}>
                          {cell === null || cell === undefined ? "NULL" : String(cell)}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}
        </section>

        {message ? (
          <p role="status" style={{ marginTop: 12, padding: 8, background: "#eef2ff", borderRadius: 4 }}>
            {message}
          </p>
        ) : null}
      </div>
    </div>
  );
}
