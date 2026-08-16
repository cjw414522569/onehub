import { useCallback, useEffect, useState } from "react";
import {
  agentList,
  agentProjectFiles,
  agentStart,
  agentStop,
} from "../../shared/tauri/commands";
import type {
  AgentInfo,
  AgentProjectEntry,
  AgentProjectFilesResult,
} from "./agentTypes";

interface AgentHubPanelProps {
  open: boolean;
  onClose: () => void;
}

export function AgentHubPanel({ open, onClose }: AgentHubPanelProps) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [project, setProject] = useState<AgentProjectFilesResult | null>(null);
  const [projectPath, setProjectPath] = useState("");
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshAgents = useCallback(async () => {
    try {
      const result = await agentList();
      setAgents(result);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }, []);

  const loadProject = useCallback(async (relative?: string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await agentProjectFiles(projectPath || undefined, relative);
      setProject(result);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, [projectPath]);

  useEffect(() => {
    if (!open) {
      return;
    }
    void refreshAgents();
    void loadProject();
  }, [open, refreshAgents, loadProject]);

  const startAgent = async (kind: string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await agentStart(kind);
      setMessage(`Agent 已启动：${result.id}`);
      await refreshAgents();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const stopAgent = async (id: string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await agentStop(id);
      setMessage(result.stopped ? `已停止 Agent ${id}。` : `Agent ${id} 未在运行。`);
      await refreshAgents();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return null;
  }

  const renderEntry = (entry: AgentProjectEntry, depth: number) => (
    <li
      key={entry.relative}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 4px", fontSize: 12 }}
    >
      <span style={{ width: depth * 12 }} />
      <button
        type="button"
        onClick={() => {
          if (entry.type === "directory") {
            void loadProject(entry.relative);
          }
        }}
        style={{
          border: "none",
          background: "transparent",
          cursor: entry.type === "directory" ? "pointer" : "default",
          textAlign: "left",
          fontFamily: "monospace",
          fontSize: 12,
          display: "flex",
          alignItems: "center",
          gap: 4,
          flex: 1,
        }}
      >
        <span>{entry.type === "directory" ? "📁" : "📄"}</span>
        {entry.name}
        {entry.type === "file" ? (
          <span style={{ color: "#6b7280", marginLeft: "auto" }}>{entry.size} B</span>
        ) : null}
      </button>
    </li>
  );

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Agent Hub"
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
          width: 900,
          maxWidth: "94vw",
          height: "86vh",
          maxHeight: "86vh",
          background: "#f5f6f8",
          color: "#1f2328",
          borderRadius: 8,
          padding: 16,
          boxShadow: "0 8px 32px rgba(0,0,0,0.25)",
          fontFamily: "system-ui, sans-serif",
          fontSize: 13,
          display: "flex",
          flexDirection: "column",
          boxSizing: "border-box",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
          <h2 style={{ margin: 0, fontSize: 16 }}>Agent Hub</h2>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <button type="button" onClick={() => void refreshAgents()} disabled={busy}>
              刷新 Agent
            </button>
            <button
              type="button"
              onClick={onClose}
              aria-label="关闭"
              style={{ border: "none", background: "transparent", fontSize: 20, cursor: "pointer", lineHeight: 1 }}
            >
              ×
            </button>
          </div>
        </div>

        <div style={{ display: "flex", flex: 1, minHeight: 0, gap: 12 }}>
          <section style={{ width: 300, border: "1px solid #d1d5db", borderRadius: 4, display: "flex", flexDirection: "column", background: "#ffffff", flexShrink: 0 }}>
            <div style={{ padding: "6px 8px", borderBottom: "1px solid #eef0f3", fontSize: 12, fontWeight: 600 }}>
              终端 Agent（{agents.filter((a) => a.terminal).length} 运行中 / {agents.length} 总数）
            </div>
            <div style={{ flex: 1, overflow: "auto" }}>
              {agents.length === 0 ? (
                <p style={{ margin: 8, color: "#6b7280", fontSize: 12 }}>暂无 Agent。</p>
              ) : (
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {agents.map((agent) => (
                    <li
                      key={agent.id}
                      style={{
                        borderBottom: "1px solid #eef0f3",
                        padding: "6px 8px",
                        background: selectedAgent === agent.id ? "#e8f0fe" : "transparent",
                      }}
                    >
                      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                        <button
                          type="button"
                          onClick={() => setSelectedAgent(agent.id)}
                          style={{ border: "none", background: "transparent", cursor: "pointer", textAlign: "left", fontSize: 12 }}
                        >
                          <strong>{agent.name}</strong>
                          <span style={{ display: "block", color: "#6b7280" }}>
                            {agent.kind} · {agent.status}
                          </span>
                        </button>
                        {agent.terminal ? (
                          <button type="button" onClick={() => void stopAgent(agent.id)} disabled={busy} style={{ fontSize: 11 }}>
                            停止
                          </button>
                        ) : (
                          <button type="button" onClick={() => void startAgent(agent.kind)} disabled={busy} style={{ fontSize: 11 }}>
                            启动
                          </button>
                        )}
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>

          <section style={{ flex: 1, border: "1px solid #d1d5db", borderRadius: 4, display: "flex", flexDirection: "column", background: "#ffffff", minWidth: 0 }}>
            <div style={{ padding: "6px 8px", borderBottom: "1px solid #eef0f3", fontSize: 12, fontWeight: 600 }}>
              项目文件树
            </div>
            <div style={{ display: "flex", gap: 6, padding: "6px 8px", borderBottom: "1px solid #eef0f3" }}>
              <input
                value={projectPath}
                onChange={(event) => setProjectPath(event.target.value)}
                placeholder="项目根目录（留空用当前目录）"
                style={{ flex: 1, fontSize: 12, fontFamily: "monospace" }}
              />
              <button type="button" onClick={() => void loadProject()} disabled={busy}>
                打开
              </button>
              {project && project.relative ? (
                <button type="button" onClick={() => void loadProject("")} disabled={busy}>
                  返回根
                </button>
              ) : null}
            </div>
            <div style={{ flex: 1, overflow: "auto", padding: 4 }}>
              <p style={{ margin: "2px 6px", color: "#6b7280", fontSize: 11, wordBreak: "break-all" }}>
                根：{project?.root || "…"}
                {project?.relative ? ` / ${project.relative}` : ""}
              </p>
              {project && project.entries.length === 0 ? (
                <p style={{ margin: 8, color: "#6b7280", fontSize: 12 }}>目录为空。</p>
              ) : (
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {project?.entries.map((entry) => renderEntry(entry, 0))}
                </ul>
              )}
            </div>
          </section>
        </div>

        {message ? (
          <p role="status" style={{ marginTop: 10, padding: 8, background: "#eef2ff", borderRadius: 4 }}>
            {message}
          </p>
        ) : null}
      </div>
    </div>
  );
}