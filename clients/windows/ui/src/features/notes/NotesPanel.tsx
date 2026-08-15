import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import * as monaco from "monaco-editor";
import { notesDelete, notesList, notesRead, notesSave } from "../../shared/tauri/commands";

const loadMarkdownPreview = () => import("./MarkdownPreview");
const MarkdownPreview = lazy(loadMarkdownPreview);

interface NotesPanelProps {
  open: boolean;
  onClose: () => void;
}

export function NotesPanel({ open, onClose }: NotesPanelProps) {
  const [notes, setNotes] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [mode, setMode] = useState<"edit" | "preview" | "split">("split");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef(content);
  const activeRef = useRef(active);

  useEffect(() => {
    contentRef.current = content;
  }, [content]);
  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  useEffect(() => {
    if (!open) {
      return;
    }
    void refreshList();
  }, [open]);

  useEffect(() => {
    if (!open || !containerRef.current || editorRef.current) {
      return;
    }
    const editor = monaco.editor.create(containerRef.current, {
      value: contentRef.current,
      language: "markdown",
      theme: "vs",
      fontSize: 13,
      minimap: { enabled: false },
      automaticLayout: true,
      wordWrap: "on",
      scrollBeyondLastLine: false,
    });
    editor.onDidChangeModelContent(() => {
      const next = editor.getValue();
      contentRef.current = next;
      setContent(next);
      setDirty(true);
    });
    editorRef.current = editor;
    return () => {
      editor.dispose();
      editorRef.current = null;
    };
  }, [open]);

  const refreshList = useCallback(async () => {
    try {
      const result = await notesList();
      setNotes(result.notes);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }, []);

  const openNote = async (name: string) => {
    setBusy(true);
    setMessage(null);
    try {
      if (activeRef.current && dirty) {
        await notesSave(activeRef.current, contentRef.current);
      }
      const result = await notesRead(name);
      setActive(result.name);
      setContent(result.content);
      contentRef.current = result.content;
      setDirty(false);
      if (editorRef.current) {
        editorRef.current.setValue(result.content);
      }
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const newNote = async () => {
    const name = window.prompt("新笔记名：");
    if (!name || !name.trim()) {
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      await notesSave(name.trim(), "");
      await refreshList();
      await openNote(name.trim());
      setMessage(`已创建笔记 ${name.trim()}。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!active) {
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      await notesSave(active, content);
      setDirty(false);
      setMessage(`已保存 ${active}。`);
      await refreshList();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!active) {
      return;
    }
    if (!window.confirm(`删除笔记 ${active}？`)) {
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      await notesDelete(active);
      setActive(null);
      setContent("");
      contentRef.current = "";
      setDirty(false);
      await refreshList();
      setMessage("已删除。");
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
      aria-label="Markdown 笔记"
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
          height: "88vh",
          maxHeight: "88vh",
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
          <h2 style={{ margin: 0, fontSize: 16 }}>Markdown 笔记</h2>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <button type="button" onClick={newNote} disabled={busy}>
              新建
            </button>
            <button type="button" onClick={() => void save()} disabled={busy || !active || !dirty}>
              保存{dirty ? " ●" : ""}
            </button>
            <button type="button" onClick={() => void remove()} disabled={busy || !active} style={{ color: "#b91c1c" }}>
              删除
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
          <aside style={{ width: 180, border: "1px solid #d1d5db", borderRadius: 4, overflow: "auto", background: "#ffffff", flexShrink: 0 }}>
            <div style={{ padding: "6px 8px", borderBottom: "1px solid #eef0f3", fontSize: 12, fontWeight: 600 }}>
              笔记列表（{notes.length}）
            </div>
            {notes.length === 0 ? (
              <p style={{ margin: 8, color: "#6b7280", fontSize: 12 }}>暂无笔记，点击“新建”。</p>
            ) : (
              <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                {notes.map((name) => (
                  <li key={name} style={{ borderBottom: "1px solid #eef0f3" }}>
                    <button
                      type="button"
                      onClick={() => void openNote(name)}
                      style={{
                        width: "100%",
                        textAlign: "left",
                        background: name === active ? "#e8f0fe" : "transparent",
                        border: "none",
                        padding: "6px 8px",
                        cursor: "pointer",
                        fontSize: 12,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {name}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </aside>

          <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6 }}>
              <strong style={{ fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {active || "（未选择笔记）"}
              </strong>
              <span style={{ flex: 1 }} />
              {(["edit", "split", "preview"] as const).map((m) => (
                <button
                  key={m}
                  type="button"
                  onClick={() => setMode(m)}
                  style={{
                    background: mode === m ? "#2374c6" : "#ffffff",
                    color: mode === m ? "#ffffff" : "#1f2328",
                    border: "1px solid #d1d5db",
                    borderRadius: 4,
                    padding: "2px 8px",
                    fontSize: 12,
                    cursor: "pointer",
                  }}
                >
                  {m === "edit" ? "编辑" : m === "preview" ? "预览" : "分栏"}
                </button>
              ))}
            </div>
            <div style={{ flex: 1, display: "flex", minHeight: 0, gap: 8 }}>
              {mode !== "preview" ? (
                <div
                  ref={containerRef}
                  style={{ flex: mode === "edit" ? 1 : 1, minWidth: 0, border: "1px solid #d1d5db", borderRadius: 4, background: "#ffffff" }}
                />
              ) : null}
              {mode === "split" ? <div style={{ width: 1, background: "#d1d5db" }} /> : null}
              {mode !== "edit" ? (
                <div style={{ flex: mode === "preview" ? 1 : 1, minWidth: 0, border: "1px solid #d1d5db", borderRadius: 4, background: "#ffffff", overflow: "hidden" }}>
                  <Suspense fallback={<p style={{ padding: 8, color: "#6b7280" }}>加载预览…</p>}>
                    <MarkdownPreview markdown={content} />
                  </Suspense>
                </div>
              ) : null}
            </div>
          </div>
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