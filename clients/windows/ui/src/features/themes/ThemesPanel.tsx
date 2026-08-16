import { useCallback, useEffect, useState } from "react";
import {
  themeApply,
  themeGetAccent,
  themeImport,
  themeList,
  themeSetAccent,
  windowGetAlpha,
  windowSetAlpha,
} from "../../shared/tauri/commands";
import type { ThemeInfo } from "./themesTypes";

interface ThemesPanelProps {
  open: boolean;
  onClose: () => void;
}

export function ThemesPanel({ open, onClose }: ThemesPanelProps) {
  const [themes, setThemes] = useState<ThemeInfo[]>([]);
  const [activeId, setActiveId] = useState("");
  const [accent, setAccent] = useState("#2374c6");
  const [alpha, setAlpha] = useState(100);
  const [importText, setImportText] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [themeResult, accentResult, alphaResult] = await Promise.all([
        themeList(),
        themeGetAccent(),
        windowGetAlpha(),
      ]);
      setThemes(themeResult.themes);
      setActiveId(themeResult.active);
      setAccent(accentResult.accent);
      setAlpha(alphaResult.alpha);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => {
    if (open) {
      void refresh();
    }
  }, [open, refresh]);

  const importTheme = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await themeImport(importText);
      setMessage(`已导入主题：${result.name}。`);
      setImportText("");
      await refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const applyTheme = async (id: string) => {
    setBusy(true);
    setMessage(null);
    try {
      await themeApply(id);
      setActiveId(id);
      setMessage(`已应用主题 ${id}。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const changeAccent = async (color: string) => {
    setAccent(color);
    try {
      await themeSetAccent(color);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const changeAlpha = async (value: number) => {
    setAlpha(value);
    try {
      await windowSetAlpha(value);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  if (!open) {
    return null;
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="主题与外观"
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
          <h2 style={{ margin: 0, fontSize: 16 }}>主题与外观</h2>
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
          <h3 style={{ margin: "0 0 6px", fontSize: 13 }}>应用/终端主题</h3>
          <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
            {themes.map((theme) => (
              <li
                key={theme.id}
                style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px", border: "1px solid #e5e7eb", borderRadius: 6, marginBottom: 6, background: "#ffffff" }}
              >
                <span
                  style={{ width: 18, height: 18, borderRadius: 4, background: theme.accent, border: "1px solid #d1d5db", display: "inline-block" }}
                />
                <div style={{ flex: 1 }}>
                  <strong>{theme.name}</strong>
                  <span style={{ display: "block", color: "#6b7280", fontSize: 11, fontFamily: "monospace" }}>
                    {theme.accent} · 背景 {theme.background}
                  </span>
                </div>
                {activeId === theme.id ? (
                  <span style={{ fontSize: 12, color: "#16a34a" }}>当前</span>
                ) : (
                  <button type="button" onClick={() => void applyTheme(theme.id)} disabled={busy} style={{ fontSize: 11 }}>
                    应用
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>

        <section style={{ marginTop: 12, borderTop: "1px solid #e5e7eb", paddingTop: 10 }}>
          <h3 style={{ margin: "0 0 6px", fontSize: 13 }}>导入主题（JSON）</h3>
          <textarea
            value={importText}
            onChange={(event) => setImportText(event.target.value)}
            rows={4}
            placeholder='{"name": "OneDark", "accent": "#61afef", "background": "#282c34", "foreground": "#abb2bf", "terminal": {"background": "#21252b"}}'
            style={{ width: "100%", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11 }}
          />
          <button type="button" onClick={() => void importTheme()} disabled={busy || !importText.trim()} style={{ marginTop: 6 }}>
            导入
          </button>
        </section>

        <section style={{ marginTop: 12, borderTop: "1px solid #e5e7eb", paddingTop: 10 }}>
          <h3 style={{ margin: "0 0 6px", fontSize: 13 }}>强调色与窗口透明度</h3>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <label style={{ display: "grid", gap: 4, fontSize: 12 }}>
              强调色
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <input type="color" value={accent} onChange={(event) => void changeAccent(event.target.value)} style={{ width: 40, height: 28, border: "none", background: "transparent" }} />
                <code>{accent}</code>
              </div>
            </label>
            <label style={{ display: "grid", gap: 4, fontSize: 12 }}>
              窗口透明度：{alpha}%
              <input type="range" min={0} max={100} value={alpha} onChange={(event) => void changeAlpha(Number(event.target.value))} />
            </label>
          </div>
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