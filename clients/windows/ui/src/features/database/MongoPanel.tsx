import { useCallback, useState } from "react";
import {
  dbConnectionConnect,
  dbConnectionDisconnect,
  dbConnectionTest,
  mongodbCollections,
  mongodbDelete,
  mongodbDocuments,
  mongodbInsert,
  mongodbUpdate,
} from "../../shared/tauri/commands";
import type {
  DbConnectionInput,
  DbTestResult,
  MongoCollectionsResult,
  MongoDocumentsResult,
} from "./dbTypes";

interface MongoPanelProps {
  open: boolean;
  onClose: () => void;
}

function emptyForm(): DbConnectionInput {
  return {
    engine: "mongodb",
    name: "MongoDB",
    host: "127.0.0.1",
    port: 27017,
    username: "",
    password: "",
    database: "test",
    ssl: false,
  };
}

export function MongoPanel({ open, onClose }: MongoPanelProps) {
  const [form, setForm] = useState<DbConnectionInput>(emptyForm());
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [collections, setCollections] = useState<string[]>([]);
  const [selectedCollection, setSelectedCollection] = useState<string | null>(null);
  const [filterJson, setFilterJson] = useState("{}");
  const [limit, setLimit] = useState(50);
  const [documents, setDocuments] = useState<unknown[]>([]);
  const [insertJson, setInsertJson] = useState("{\"name\": \"onehub\", \"value\": 1}");
  const [updateJson, setUpdateJson] = useState("{\"$set\": {\"value\": 2}}");
  const [deleteFilter, setDeleteFilter] = useState("{}");

  const connect = useCallback(async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await dbConnectionConnect(form);
      setSessionId(result.session_id);
      setMessage(`MongoDB 会话已建立：${result.session_id}`);
      void refreshCollections(result.session_id);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, [form]);

  const testNow = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result: DbTestResult = await dbConnectionTest(form);
      setMessage(result.message);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    if (sessionId) {
      await dbConnectionDisconnect(sessionId).catch(() => {});
    }
    setSessionId(null);
    setCollections([]);
    setSelectedCollection(null);
    setDocuments([]);
    setMessage("已断开。");
  };

  const refreshCollections = async (sid: string | null) => {
    const target = sid || sessionId;
    if (!target) {
      setMessage("请先连接 MongoDB。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result: MongoCollectionsResult = await mongodbCollections(target);
      setCollections(result.collections);
      setMessage(`已加载 ${result.collections.length} 个集合（库 ${result.database}）。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const queryDocuments = async (collection = selectedCollection) => {
    if (!sessionId || !collection) {
      setMessage("请先连接并选择集合。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result: MongoDocumentsResult = await mongodbDocuments(sessionId, collection, filterJson, limit);
      setDocuments(result.documents);
      setMessage(`查询完成：${result.count} 条文档。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const insertDocument = async () => {
    if (!sessionId || !selectedCollection) {
      setMessage("请先连接并选择集合。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result = await mongodbInsert(sessionId, selectedCollection, insertJson);
      setMessage(`已插入，_id=${JSON.stringify(result.inserted_id)}。`);
      void queryDocuments();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const updateDocument = async () => {
    if (!sessionId || !selectedCollection) {
      setMessage("请先连接并选择集合。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result = await mongodbUpdate(sessionId, selectedCollection, filterJson, updateJson);
      setMessage(`更新完成：匹配 ${result.matched}，修改 ${result.modified}。`);
      void queryDocuments();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const deleteDocument = async () => {
    if (!sessionId || !selectedCollection) {
      setMessage("请先连接并选择集合。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result = await mongodbDelete(sessionId, selectedCollection, deleteFilter);
      setMessage(`已删除 ${result.deleted} 条文档。`);
      void queryDocuments();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return null;
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="MongoDB 控制台"
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 2100,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.35)",
      }}
    >
      <div
        style={{
          width: 860,
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
          <h2 style={{ margin: 0, fontSize: 16 }}>MongoDB 控制台</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            style={{ border: "none", background: "transparent", fontSize: 20, cursor: "pointer", lineHeight: 1 }}
          >
            ×
          </button>
        </div>

        <section style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr 1fr", gap: 8 }}>
          <label style={{ display: "grid", gap: 2 }}>
            主机
            <input value={form.host || ""} onChange={(event) => setForm({ ...form, host: event.target.value })} />
          </label>
          <label style={{ display: "grid", gap: 2 }}>
            端口
            <input type="number" value={form.port ?? 27017} onChange={(event) => setForm({ ...form, port: Number(event.target.value) })} />
          </label>
          <label style={{ display: "grid", gap: 2 }}>
            用户名（可选）
            <input value={form.username || ""} onChange={(event) => setForm({ ...form, username: event.target.value })} />
          </label>
          <label style={{ display: "grid", gap: 2 }}>
            密码
            <input type="password" value={form.password || ""} onChange={(event) => setForm({ ...form, password: event.target.value })} />
          </label>
          <label style={{ display: "grid", gap: 2 }}>
            数据库
            <input value={form.database || "test"} onChange={(event) => setForm({ ...form, database: event.target.value })} />
          </label>
        </section>
        <div style={{ display: "flex", gap: 8, marginTop: 10, alignItems: "center" }}>
          <button type="button" onClick={() => void testNow()} disabled={busy}>
            测试连接
          </button>
          <button type="button" onClick={() => void connect()} disabled={busy || !form.host}>
            {sessionId ? "重连" : "连接"}
          </button>
          <button type="button" onClick={() => void disconnect()} disabled={!sessionId}>
            断开
          </button>
          {sessionId ? <span style={{ fontSize: 12, color: "#16a34a" }}>已连接</span> : null}
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "240px 1fr", gap: 16, marginTop: 14 }}>
          <section>
            <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>集合{sessionId ? "（已连接）" : ""}</h3>
            <button type="button" onClick={() => void refreshCollections(sessionId)} disabled={busy || !sessionId} style={{ marginBottom: 6 }}>
              刷新集合
            </button>
            <div style={{ border: "1px solid #d1d5db", borderRadius: 4, maxHeight: 320, overflow: "auto", background: "#ffffff" }}>
              {collections.length === 0 ? (
                <p style={{ margin: 8, color: "#6b7280" }}>暂无集合。</p>
              ) : (
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {collections.map((name) => (
                    <li key={name} style={{ borderBottom: "1px solid #eef0f3" }}>
                      <button
                        type="button"
                        onClick={() => {
                          setSelectedCollection(name);
                          void queryDocuments(name);
                        }}
                        style={{
                          width: "100%",
                          textAlign: "left",
                          background: name === selectedCollection ? "#e8f0fe" : "transparent",
                          border: "none",
                          padding: "5px 8px",
                          cursor: "pointer",
                          fontFamily: "monospace",
                          fontSize: 12,
                        }}
                      >
                        {name}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>

          <section>
            <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>文档查询{selectedCollection ? `（${selectedCollection}）` : ""}</h3>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 120px", gap: 6, marginBottom: 6 }}>
              <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
                过滤条件（JSON）
                <input value={filterJson} onChange={(event) => setFilterJson(event.target.value)} placeholder='{"name": "onehub"}' style={{ fontFamily: "monospace" }} />
              </label>
              <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
                条数上限
                <input type="number" value={limit} onChange={(event) => setLimit(Number(event.target.value))} />
              </label>
            </div>
            <button type="button" onClick={() => void queryDocuments()} disabled={busy || !sessionId || !selectedCollection}>
              查询
            </button>
            <div style={{ marginTop: 8, border: "1px solid #d1d5db", borderRadius: 4, maxHeight: 260, overflow: "auto", background: "#ffffff", padding: 6 }}>
              {documents.length === 0 ? (
                <p style={{ margin: 0, color: "#6b7280", fontSize: 12 }}>暂无文档。</p>
              ) : (
                documents.map((doc, index) => (
                  <pre key={index} style={{ margin: "0 0 6px", fontSize: 11, borderBottom: "1px solid #eef0f3", paddingBottom: 6 }}>
                    {JSON.stringify(doc, null, 2)}
                  </pre>
                ))
              )}
            </div>

            <div style={{ marginTop: 10, borderTop: "1px solid #e5e7eb", paddingTop: 8 }}>
              <h4 style={{ margin: "0 0 6px", fontSize: 12 }}>插入文档</h4>
              <textarea value={insertJson} onChange={(event) => setInsertJson(event.target.value)} rows={2} style={{ width: "100%", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11 }} />
              <button type="button" onClick={() => void insertDocument()} disabled={busy || !sessionId || !selectedCollection} style={{ marginTop: 4 }}>
                插入
              </button>
            </div>

            <div style={{ marginTop: 10, borderTop: "1px solid #e5e7eb", paddingTop: 8 }}>
              <h4 style={{ margin: "0 0 6px", fontSize: 12 }}>更新 / 删除</h4>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
                <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
                  更新语句（JSON）
                  <input value={updateJson} onChange={(event) => setUpdateJson(event.target.value)} style={{ fontFamily: "monospace" }} />
                </label>
                <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
                  删除条件（JSON）
                  <input value={deleteFilter} onChange={(event) => setDeleteFilter(event.target.value)} style={{ fontFamily: "monospace" }} />
                </label>
              </div>
              <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                <button type="button" onClick={() => void updateDocument()} disabled={busy || !sessionId || !selectedCollection}>
                  更新首条匹配
                </button>
                <button type="button" onClick={() => void deleteDocument()} disabled={busy || !sessionId || !selectedCollection} style={{ color: "#b91c1c" }}>
                  删除首条匹配
                </button>
              </div>
            </div>
          </section>
        </div>

        {message ? (
          <p role="status" style={{ marginTop: 12, padding: 8, background: "#eef2ff", borderRadius: 4 }}>
            {message}
          </p>
        ) : null}
      </div>
    </div>
  );
}