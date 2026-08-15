import { useCallback, useState } from "react";
import {
  dbConnectionConnect,
  dbConnectionDisconnect,
  dbConnectionTest,
  redisConsole,
  redisDel,
  redisGet,
  redisKeys,
  redisSet,
  redisTtl,
  redisType,
} from "../../shared/tauri/commands";
import type {
  DbConnectionInput,
  DbTestResult,
  RedisConsoleResult,
  RedisKeyList,
  RedisSetResult,
} from "./dbTypes";

interface RedisPanelProps {
  open: boolean;
  onClose: () => void;
}

function emptyForm(): DbConnectionInput {
  return {
    engine: "redis",
    name: "Redis",
    host: "127.0.0.1",
    port: 6379,
    username: "",
    password: "",
    database: "0",
    ssl: false,
  };
}

export function RedisPanel({ open, onClose }: RedisPanelProps) {
  const [form, setForm] = useState<DbConnectionInput>(emptyForm());
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [pattern, setPattern] = useState("*");
  const [keys, setKeys] = useState<string[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [valueText, setValueText] = useState("");
  const [ttlText, setTtlText] = useState("");
  const [keyType, setKeyType] = useState("");
  const [consoleInput, setConsoleInput] = useState("");
  const [consoleResult, setConsoleResult] = useState<RedisConsoleResult | null>(null);

  const connect = useCallback(async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await dbConnectionConnect(form);
      setSessionId(result.session_id);
      setMessage(`Redis 会话已建立：${result.session_id}`);
      void refreshKeys(result.session_id);
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
    setKeys([]);
    setSelectedKey(null);
    setValueText("");
    setTtlText("");
    setKeyType("");
    setMessage("已断开。");
  };

  const refreshKeys = async (sid: string | null) => {
    const target = sid || sessionId;
    if (!target) {
      setMessage("请先连接 Redis。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result: RedisKeyList = await redisKeys(target, pattern || "*");
      setKeys(result.keys);
      setMessage(`已加载 ${result.keys.length} 个键（模式 ${result.pattern}）。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const selectKey = async (key: string) => {
    if (!sessionId) {
      return;
    }
    setSelectedKey(key);
    setBusy(true);
    setMessage(null);
    try {
      const [value, ttl, kind] = await Promise.all([
        redisGet(sessionId, key),
        redisTtl(sessionId, key),
        redisType(sessionId, key),
      ]);
      setValueText(value.value === null || value.value === undefined ? "" : String(value.value));
      setTtlText(ttl.ttl_seconds >= 0 ? String(ttl.ttl_seconds) : "");
      setKeyType(kind.type);
      setMessage(`键 ${key}（类型 ${kind.type}，TTL ${ttl.ttl_seconds}s）。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const saveValue = async () => {
    if (!sessionId || !selectedKey) {
      setMessage("请先选择键。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const ttl = ttlText.trim() === "" ? undefined : Number(ttlText);
      const result: RedisSetResult = await redisSet(sessionId, selectedKey, valueText, ttl);
      setMessage(result.ok ? `已保存 ${selectedKey}。` : "保存失败。");
      void refreshKeys(sessionId);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const deleteKey = async () => {
    if (!sessionId || !selectedKey) {
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result = await redisDel(sessionId, selectedKey);
      setSelectedKey(null);
      setValueText("");
      setTtlText("");
      setKeyType("");
      setMessage(`已删除 ${selectedKey}（${result.removed} 个）。`);
      void refreshKeys(sessionId);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const runConsole = async () => {
    if (!sessionId) {
      setMessage("请先连接 Redis。");
      return;
    }
    if (!consoleInput.trim()) {
      setMessage("请输入命令。");
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const result = await redisConsole(sessionId, consoleInput.trim());
      setConsoleResult(result);
      setMessage("命令已执行。");
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
      aria-label="Redis 控制台"
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
          width: 820,
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
          <h2 style={{ margin: 0, fontSize: 16 }}>Redis 控制台</h2>
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
            <input type="number" value={form.port ?? 6379} onChange={(event) => setForm({ ...form, port: Number(event.target.value) })} />
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
            DB 编号
            <input value={form.database || "0"} onChange={(event) => setForm({ ...form, database: event.target.value })} />
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

        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, marginTop: 14 }}>
          <section>
            <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>键浏览{sessionId ? "（已连接）" : ""}</h3>
            <div style={{ display: "flex", gap: 6, marginBottom: 6 }}>
              <input value={pattern} onChange={(event) => setPattern(event.target.value)} placeholder="模式（*）" style={{ flex: 1 }} />
              <button type="button" onClick={() => void refreshKeys(sessionId)} disabled={busy || !sessionId}>
                刷新
              </button>
            </div>
            <div style={{ border: "1px solid #d1d5db", borderRadius: 4, maxHeight: 220, overflow: "auto", background: "#ffffff" }}>
              {keys.length === 0 ? (
                <p style={{ margin: 8, color: "#6b7280" }}>暂无键。</p>
              ) : (
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {keys.map((key) => (
                    <li key={key} style={{ borderBottom: "1px solid #eef0f3" }}>
                      <button
                        type="button"
                        onClick={() => void selectKey(key)}
                        style={{
                          width: "100%",
                          textAlign: "left",
                          background: key === selectedKey ? "#e8f0fe" : "transparent",
                          border: "none",
                          padding: "5px 8px",
                          cursor: "pointer",
                          fontFamily: "monospace",
                          fontSize: 12,
                        }}
                      >
                        {key}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>

          <section>
            <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>值编辑{selectedKey ? `（${selectedKey}${keyType ? ` · ${keyType}` : ""}）` : ""}</h3>
            <div style={{ display: "grid", gap: 6 }}>
              <textarea
                value={valueText}
                onChange={(event) => setValueText(event.target.value)}
                rows={6}
                disabled={!selectedKey}
                placeholder="键的值"
                style={{ width: "100%", boxSizing: "border-box", fontFamily: "monospace", fontSize: 12 }}
              />
              <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
                TTL（秒，留空为永久）
                <input value={ttlText} onChange={(event) => setTtlText(event.target.value)} disabled={!selectedKey} placeholder="-1 或留空" />
              </label>
              <div style={{ display: "flex", gap: 8 }}>
                <button type="button" onClick={() => void saveValue()} disabled={busy || !selectedKey}>
                  保存值
                </button>
                <button type="button" onClick={() => void deleteKey()} disabled={busy || !selectedKey} style={{ color: "#b91c1c" }}>
                  删除键
                </button>
              </div>
            </div>
          </section>
        </div>

        <section style={{ marginTop: 14 }}>
          <h3 style={{ margin: "0 0 8px", fontSize: 13 }}>命令行 Console{sessionId ? "（已连接）" : ""}</h3>
          <div style={{ display: "flex", gap: 6 }}>
            <input
              value={consoleInput}
              onChange={(event) => setConsoleInput(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void runConsole();
                }
              }}
              placeholder='例如：GET user:1 / INFO / SCAN 0 MATCH user:*'
              style={{ flex: 1, fontFamily: "monospace" }}
              disabled={!sessionId}
            />
            <button type="button" onClick={() => void runConsole()} disabled={busy || !sessionId || !consoleInput.trim()}>
              执行
            </button>
          </div>
          {consoleResult ? (
            <pre style={{ marginTop: 8, maxHeight: 160, overflow: "auto", background: "#ffffff", border: "1px solid #d1d5db", borderRadius: 4, padding: 6, fontSize: 11 }}>
              {JSON.stringify(consoleResult.rows, null, 2)}
            </pre>
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