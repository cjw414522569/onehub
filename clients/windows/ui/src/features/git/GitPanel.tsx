import { useCallback, useEffect, useState } from "react";
import {
  gitBranches,
  gitDiff,
  gitStatus,
  gitSwitch,
} from "../../shared/tauri/commands";
import type {
  GitBranch,
  GitDiffHunk,
} from "./gitTypes";

interface GitPanelProps {
  open: boolean;
  onClose: () => void;
}

interface DiffRow {
  kind: string;
  oldLine: number | null;
  newLine: number | null;
  text: string;
}

export function GitPanel({ open, onClose }: GitPanelProps) {
  const [repo, setRepo] = useState("");
  const [branches, setBranches] = useState<GitBranch[]>([]);
  const [currentBranch, setCurrentBranch] = useState("");
  const [changes, setChanges] = useState<{ path: string; status: string }[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [rows, setRows] = useState<DiffRow[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadRepo = useCallback(async (repoPath: string) => {
    if (!repoPath.trim()) {
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const [branchResult, statusResult] = await Promise.all([
        gitBranches(repoPath),
        gitStatus(repoPath),
      ]);
      setBranches(branchResult.branches);
      setCurrentBranch(branchResult.current || "");
      setChanges(statusResult.entries);
      setSelectedFile(null);
      setRows([]);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (open && repo) {
      void loadRepo(repo);
    }
  }, [open, repo, loadRepo]);

  const openFile = async (file: string) => {
    setSelectedFile(file);
    setBusy(true);
    setMessage(null);
    try {
      const result = await gitDiff(repo, file);
      setRows(buildDiffRows(result.hunks));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const switchBranch = async (branch: string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await gitSwitch(repo, branch);
      setMessage(`已切换到 ${result.branch}。`);
      await loadRepo(repo);
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
      aria-label="Git 分支与 Diff"
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
          width: 980,
          maxWidth: "96vw",
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
          <h2 style={{ margin: 0, fontSize: 16 }}>Git 分支与 Diff</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            style={{ border: "none", background: "transparent", fontSize: 20, cursor: "pointer", lineHeight: 1 }}
          >
            ×
          </button>
        </div>

        <div style={{ display: "flex", gap: 6, marginBottom: 10 }}>
          <input
            value={repo}
            onChange={(event) => setRepo(event.target.value)}
            placeholder="仓库路径（如 C:\work\ssh）"
            style={{ flex: 1, fontFamily: "monospace", fontSize: 12 }}
          />
          <button type="button" onClick={() => void loadRepo(repo)} disabled={busy || !repo.trim()}>
            加载
          </button>
        </div>

        <div style={{ display: "flex", flex: 1, minHeight: 0, gap: 12 }}>
          <section style={{ width: 240, border: "1px solid #d1d5db", borderRadius: 4, display: "flex", flexDirection: "column", background: "#ffffff", flexShrink: 0 }}>
            <div style={{ padding: "6px 8px", borderBottom: "1px solid #eef0f3", fontSize: 12, fontWeight: 600 }}>
              分支{currentBranch ? `（当前：${currentBranch}）` : ""}
            </div>
            <div style={{ flex: 1, overflow: "auto" }}>
              {branches.length === 0 ? (
                <p style={{ margin: 8, color: "#6b7280", fontSize: 12 }}>暂无分支。</p>
              ) : (
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {branches.map((branch) => (
                    <li key={branch.name} style={{ borderBottom: "1px solid #eef0f3" }}>
                      <button
                        type="button"
                        onClick={() => void switchBranch(branch.name)}
                        disabled={busy || branch.current}
                        style={{
                          width: "100%",
                          textAlign: "left",
                          background: branch.current ? "#e8f0fe" : "transparent",
                          border: "none",
                          padding: "6px 8px",
                          cursor: "pointer",
                          fontSize: 12,
                        }}
                      >
                        {branch.current ? "✓ " : ""}
                        {branch.name}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>

          <section style={{ width: 280, border: "1px solid #d1d5db", borderRadius: 4, display: "flex", flexDirection: "column", background: "#ffffff", flexShrink: 0 }}>
            <div style={{ padding: "6px 8px", borderBottom: "1px solid #eef0f3", fontSize: 12, fontWeight: 600 }}>
              变更文件（{changes.length}）
            </div>
            <div style={{ flex: 1, overflow: "auto" }}>
              {changes.length === 0 ? (
                <p style={{ margin: 8, color: "#6b7280", fontSize: 12 }}>工作区干净。</p>
              ) : (
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {changes.map((change) => (
                    <li key={change.path} style={{ borderBottom: "1px solid #eef0f3" }}>
                      <button
                        type="button"
                        onClick={() => void openFile(change.path)}
                        style={{
                          width: "100%",
                          textAlign: "left",
                          background: selectedFile === change.path ? "#e8f0fe" : "transparent",
                          border: "none",
                          padding: "6px 8px",
                          cursor: "pointer",
                          fontSize: 12,
                          fontFamily: "monospace",
                          wordBreak: "break-all",
                        }}
                      >
                        <span style={{ color: change.status.startsWith("A") ? "#16a34a" : change.status.startsWith("D") ? "#b91c1c" : "#2374c6", fontWeight: 600 }}>
                          {change.status}
                        </span>{" "}
                        {change.path}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>

          <section style={{ flex: 1, border: "1px solid #d1d5db", borderRadius: 4, display: "flex", flexDirection: "column", background: "#ffffff", minWidth: 0 }}>
            <div style={{ padding: "6px 8px", borderBottom: "1px solid #eef0f3", fontSize: 12, fontWeight: 600, display: "flex", gap: 12 }}>
              <span>并排 Diff{selectedFile ? `：${selectedFile}` : ""}</span>
            </div>
            <div style={{ flex: 1, overflow: "auto", fontFamily: "monospace", fontSize: 12 }}>
              {!selectedFile ? (
                <p style={{ margin: 8, color: "#6b7280" }}>选择左侧变更文件查看并排 Diff。</p>
              ) : rows.length === 0 ? (
                <p style={{ margin: 8, color: "#6b7280" }}>无差异（文件可能仅未跟踪或内容一致）。</p>
              ) : (
                <table style={{ borderCollapse: "collapse", width: "100%" }}>
                  <thead>
                    <tr>
                      <th style={{ border: "1px solid #d1d5db", padding: "4px 8px", textAlign: "left", width: "50%", background: "#f6f8fa" }}>旧版本</th>
                      <th style={{ border: "1px solid #d1d5db", padding: "4px 8px", textAlign: "left", width: "50%", background: "#f6f8fa" }}>新版本</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((row, index) => (
                      <tr key={index}>
                        <td
                          style={{
                            border: "1px solid #e5e7eb",
                            padding: "2px 8px",
                            whiteSpace: "pre-wrap",
                            wordBreak: "break-all",
                            background: row.kind === "del" ? "#fde8e8" : row.kind === "add" ? "#e8f5e9" : "transparent",
                            color: row.kind === "del" ? "#b91c1c" : "#1f2328",
                          }}
                        >
                          {row.oldLine !== null ? `${row.oldLine} ` : "    "}
                          {row.kind === "del" || row.kind === "context" ? row.text : ""}
                        </td>
                        <td
                          style={{
                            border: "1px solid #e5e7eb",
                            padding: "2px 8px",
                            whiteSpace: "pre-wrap",
                            wordBreak: "break-all",
                            background: row.kind === "add" ? "#e8f5e9" : row.kind === "del" ? "#fde8e8" : "transparent",
                            color: row.kind === "add" ? "#166534" : "#1f2328",
                          }}
                        >
                          {row.newLine !== null ? `${row.newLine} ` : "    "}
                          {row.kind === "add" || row.kind === "context" ? row.text : ""}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
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

function buildDiffRows(hunks: GitDiffHunk[]): DiffRow[] {
  const rows: DiffRow[] = [];
  for (const hunk of hunks) {
    for (const line of hunk.lines) {
      rows.push({
        kind: line.type,
        oldLine: line.old_line || null,
        newLine: line.new_line || null,
        text: line.text,
      });
    }
  }
  return rows;
}