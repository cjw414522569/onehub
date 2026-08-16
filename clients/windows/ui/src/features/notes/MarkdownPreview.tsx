import { useEffect, useRef } from "react";
import "highlight.js/styles/github.css";
import "katex/dist/katex.min.css";
import mermaid from "mermaid";
import { notesAsset } from "../../shared/tauri/commands";
import { renderMarkdown } from "./renderMarkdown";

interface MarkdownPreviewProps {
  markdown: string;
}

export default function MarkdownPreview({ markdown }: MarkdownPreviewProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    const container = containerRef.current;
    if (!container) {
      return;
    }
    container.innerHTML = renderMarkdown(markdown);
    // Resolve note-relative media through the notes_asset bridge.
    const images = Array.from(container.querySelectorAll("img[src]")) as HTMLImageElement[];
    void Promise.all(
      images.map(async (img) => {
        const src = img.getAttribute("src") || "";
        if (src.startsWith("data:") || /^https?:/i.test(src) || src.startsWith("/")) {
          return;
        }
        try {
          const asset = await notesAsset(src);
          if (!cancelled && container.contains(img)) {
            img.src = asset.data_url;
          }
        } catch {
          // Leave the broken src; the bridge is unavailable in browser preview.
        }
      }),
    );
    // Render Mermaid diagrams.
    mermaid.initialize({ startOnLoad: false, theme: "neutral" });
    void mermaid
      .run({ nodes: Array.from(container.querySelectorAll(".mermaid")) })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [markdown]);

  return (
    <div
      ref={containerRef}
      className="markdown-preview"
      style={{
        padding: "8px 12px",
        fontFamily: "system-ui, sans-serif",
        fontSize: 14,
        lineHeight: 1.6,
        overflow: "auto",
        height: "100%",
        boxSizing: "border-box",
      }}
    />
  );
}