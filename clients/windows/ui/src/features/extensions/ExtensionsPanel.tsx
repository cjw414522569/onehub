import { useCallback, useEffect, useState } from "react";
import {
  extInstall,
  extMarketplaceList,
  extUninstall,
  extWasmCall,
  extWasmList,
  extWasmLoad,
  extWasmUnload,
} from "../../shared/tauri/commands";
import type { ExtMarketplaceProvider, ExtMarketplaceResult } from "./extensionsTypes";

interface ExtensionsPanelProps {
  open: boolean;
  onClose: () => void;
}

const CATEGORY_LABELS: Record<string, string> = {
  database: "数据库驱动",
  rdp: "RDP",
  vnc: "VNC",
  renderer: "文档渲染器",
};

export function ExtensionsPanel({ open, onClose }: ExtensionsPanelProps) {
  const [market, setMarket] = useState<ExtMarketplaceResult | null>(null);
  const [category, setCategory] = useState("all");
  const [query, setQuery] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await extMarketplaceList();
      setMarket(result);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (open) {
      void refresh();
    }
  }, [open, refresh]);

  const [wasmInput, setWasmInput] = useState("");
  const [wasmInstances, setWasmInstances] = useState<{ id: string; exports: string[] }[]>([]);
  const [wasmResult, setWasmResult] = useState<unknown>(null);

  const refreshWasmList = async () => {
    try {
      setWasmInstances(await extWasmList());
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const loadWasm = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await extWasmLoad(wasmInput.trim());
      setMessage(`WASM 已加载：${result.id}（导出 ${result.exports.join(", ")}）。`);
      setWasmInput("");
      await refreshWasmList();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const callWasm = async (handle: string, functionName: string, args: number[]) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await extWasmCall(handle, functionName, args);
      setWasmResult(result);
      setMessage(`调用完成：${JSON.stringify(result.results)}。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const unloadWasm = async (handle: string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await extWasmUnload(handle);
      setMessage(result.unloaded ? `已卸载 ${handle}。` : `${handle} 不存在。`);
      await refreshWasmList();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const toggleInstall = async (provider: ExtMarketplaceProvider) => {
    setBusy(true);
    setMessage(null);
    try {
      if (provider.installed) {
        await extUninstall(provider.id);
        setMessage(`已卸载 ${provider.name}。`);
      } else {
        await extInstall(provider.id);
        setMessage(`已安装 ${provider.name}。`);
      }
      await refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return null;
  }

  const providers = (market?.providers || []).filter((provider) => {
    if (category !== "all" && provider.category !== category) {
      return false;
    }
    const q = query.trim().toLowerCase();
    if (q && !provider.name.toLowerCase().includes(q) && !provider.id.toLowerCase().includes(q)) {
      return false;
    }
    return true;
  });

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="扩展市场"
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
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
          <h2 style={{ margin: 0, fontSize: 16 }}>扩展市场 / Provider 框架</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            style={{ border: "none", background: "transparent", fontSize: 20, cursor: "pointer", lineHeight: 1 }}
          >
            ×
          </button>
        </div>

        <div style={{ display: "flex", gap: 8, marginBottom: 10, alignItems: "center" }}>
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索扩展…"
            style={{ flex: 1, fontSize: 12 }}
          />
          <select value={category} onChange={(event) => setCategory(event.target.value)} style={{ fontSize: 12 }}>
            <option value="all">全部分类</option>
            <option value="database">数据库驱动</option>
            <option value="rdp">RDP</option>
            <option value="vnc">VNC</option>
            <option value="renderer">文档渲染器</option>
          </select>
          <button type="button" onClick={() => void refresh()} disabled={busy}>
            刷新
          </button>
        </div>

        <p style={{ margin: "0 0 8px", fontSize: 12, color: "#6b7280" }}>
          共 {providers.length} 个扩展，已安装 {market?.installed_count ?? 0} 个。
        </p>

        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {providers.map((provider) => (
            <li
              key={provider.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "8px 10px",
                border: "1px solid #e5e7eb",
                borderRadius: 6,
                marginBottom: 6,
                background: "#ffffff",
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <strong style={{ fontSize: 13 }}>{provider.name}</strong>
                <span
                  style={{
                    display: "inline-block",
                    marginLeft: 8,
                    padding: "0 6px",
                    borderRadius: 10,
                    fontSize: 11,
                    background: "#eef2ff",
                    color: "#2374c6",
                  }}
                >
                  {CATEGORY_LABELS[provider.category] || provider.category}
                </span>
                <span style={{ display: "block", color: "#6b7280", fontSize: 11, fontFamily: "monospace" }}>
                  {provider.id}
                  {provider.builtin ? " · 内置" : ""}
                </span>
              </div>
              {provider.installed ? (
                <button type="button" onClick={() => void toggleInstall(provider)} disabled={busy} style={{ fontSize: 11, color: "#b91c1c" }}>
                  卸载
                </button>
              ) : (
                <button type="button" onClick={() => void toggleInstall(provider)} disabled={busy} style={{ fontSize: 11 }}>
                  安装
                </button>
              )}
            </li>
          ))}
        </ul>

        <section style={{ marginTop: 14, borderTop: "1px solid #e5e7eb", paddingTop: 10 }}>
          <h3 style={{ margin: "0 0 6px", fontSize: 13 }}>WASM 扩展运行时（沙箱，仅暴露 host.log）</h3>
          <div style={{ display: "grid", gap: 6 }}>
            <label style={{ display: "grid", gap: 2, fontSize: 12 }}>
              WASM（base64）
              <textarea
                value={wasmInput}
                onChange={(event) => setWasmInput(event.target.value)}
                rows={2}
                placeholder="粘贴 wasm 的 base64…"
                style={{ fontFamily: "monospace", fontSize: 11 }}
              />
            </label>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <button type="button" onClick={() => void loadWasm()} disabled={busy || !wasmInput.trim()}>
                加载
              </button>
              <button type="button" onClick={() => void refreshWasmList()} disabled={busy}>
                刷新已加载
              </button>
            </div>
            {wasmInstances.length > 0 ? (
              <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                {wasmInstances.map((instance) => (
                  <li key={instance.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 6px", border: "1px solid #e5e7eb", borderRadius: 4, marginBottom: 4, background: "#ffffff" }}>
                    <code style={{ flex: 1, fontSize: 11 }}>{instance.id}</code>
                    <span style={{ fontSize: 11, color: "#6b7280" }}>{instance.exports.join(", ")}</span>
                    <button type="button" onClick={() => void callWasm(instance.id, "add", [2, 3])} disabled={busy} style={{ fontSize: 11 }}>
                      add(2,3)
                    </button>
                    <button type="button" onClick={() => void unloadWasm(instance.id)} disabled={busy} style={{ fontSize: 11, color: "#b91c1c" }}>
                      卸载
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}
            {wasmResult ? (
              <pre style={{ margin: 0, fontSize: 11, background: "#ffffff", border: "1px solid #d1d5db", borderRadius: 4, padding: 6 }}>
                {JSON.stringify(wasmResult, null, 2)}
              </pre>
            ) : null}
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