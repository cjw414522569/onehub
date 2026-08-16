import { useCallback, useEffect, useState } from "react";
import {
  acpDetectAgents,
  acpHandshake,
  acpRunTool,
} from "../../shared/tauri/commands";
import type { AcpAgentInfo, AcpHandshakeResult, AcpRunToolResult } from "./acpTypes";

interface AcpPanelProps {
  open: boolean;
  onClose: () => void;
}

export function AcpPanel({ open, onClose }: AcpPanelProps) {
  const [agents, setAgents] = useState<AcpAgentInfo[]>([]);
  const [handshakes, setHandshakes] = useState<Record<string, AcpHandshakeResult>>({});
  const [toolBinary, setToolBinary] = useState("codex");
  const [toolName, setToolName] = useState("read_file");
  const [toolArgs, setToolArgs] = useState("{\"path\": \"/tmp/a.txt\"}");
  const [toolResult, setToolResult] = useState<AcpRunToolResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const result = await acpDetectAgents();
      setAgents(result);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => {
    if (open) {
      void refresh();
    }
  }, [open, refresh]);

  const runHandshake = async (binary: string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await acpHandshake(binary);
      setHandshakes((current) => ({ ...current, [binary]: result }));
      setMessage(`握手成功：${result.agent}（协议 ${result.protocol_version || "未知"}）。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const runTool = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const argumentsValue = JSON.parse(toolArgs || "{}");
      const result = await acpRunTool(toolBinary, toolName, argumentsValue);
      setToolResult(result);
      setMessage(`工具调用已发送（${result.binary} / ${result.tool}）。`);
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
      aria-label="ACP 外部 Agent"
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
          width: 640,
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
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
          <h2 style={{ margin: 0, fontSize: 16 }}>ACP 外部 Agent（Codex / Claude Code / OpenCode）</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            style={{ border: "none", background: "transparent", fontSize: 20, cursor: "pointer", lineHeight: 1 }}
          >
            ×
          </button>
        </div>

        <section>
          <h3 style={{ margin: "0 0 6px", fontSize: 13 }}>检测 Agent</h3>
          <div style={{ display: "flex", gap: 8, marginBottom: 6 }}>
            <button type="button" onClick={() => void refresh()} disabled={busy}>
              重新检测
            </button>
          </div>
          <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
            {agents.map((agent) => (
              <li
                key={agent.binary}
                style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 8px", borderBottom: "1px solid #e5e7eb" }}
              >
                <strong style={{ width: 90 }}>{agent.label}</strong>
                <code style={{ flex: 1 }}>{agent.binary}</code>
                <span style={{ color: agent.available ? "#16a34a" : "#b91c1c", fontSize: 12 }}>
                  {agent.available ? "已安装" : "未安装"}
                </span>
                <button type="button" onClick={() => void runHandshake(agent.binary)} disabled={busy || !agent.available} style={{ fontSize: 11 }}>
                  握手
                </button>
              </li>
            ))}
          </ul>
          {Object.entries(handshakes).map(([binary, result]) => (
            <p key={binary} style={{ margin: "4px 8px", fontSize: 12, color: "#2374c6" }}>
              {binary} → {result.agent}（协议 {result.protocol_version || "未知"}）
            </p>
          ))}
        </section>

        <section style={{ marginTop: 12, borderTop: "1px solid #e5e7eb", paddingTop: 10 }}>
          <h3 style={{ margin: "0 0 6px", fontSize: 13 }}>运行工具</h3>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
            <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
              Agent
              <select value={toolBinary} onChange={(event) => setToolBinary(event.target.value)}>
                {agents.map((agent) => (
                  <option key={agent.binary} value={agent.binary}>
                    {agent.label}（{agent.binary}）
                  </option>
                ))}
              </select>
            </label>
            <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
              工具名
              <input value={toolName} onChange={(event) => setToolName(event.target.value)} />
            </label>
            <label style={{ display: "grid", gap: 2, fontSize: 12, gridColumn: "1 / -1" }}>
              参数（JSON）
              <textarea value={toolArgs} onChange={(event) => setToolArgs(event.target.value)} rows={3} style={{ fontFamily: "monospace", fontSize: 11 }} />
            </label>
          </div>
          <button type="button" onClick={() => void runTool()} disabled={busy} style={{ marginTop: 8 }}>
            运行工具
          </button>
          {toolResult ? (
            <pre style={{ marginTop: 8, maxHeight: 200, overflow: "auto", background: "#ffffff", border: "1px solid #d1d5db", borderRadius: 4, padding: 6, fontSize: 11 }}>
              {JSON.stringify(toolResult, null, 2)}
            </pre>
          ) : null}
        </section>

        {message ? (
          <p role="status" style={{ marginTop: 10, padding: 8, background: "#eef2ff", borderRadius: 4 }}>
            {message}
          </p>
        ) : null}
      </div>
    </div>
  );
}