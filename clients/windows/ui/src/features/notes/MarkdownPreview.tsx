import { useEffect, useRef } from "react";
import { marked } from "marked";
import hljs from "highlight.js";
import "highlight.js/styles/github.css";
import katex from "katex";
import "katex/dist/katex.min.css";
import mermaid from "mermaid";
import { notesAsset } from "../../shared/tauri/commands";

// Custom marked renderer: highlight code blocks (skip mermaid, which is
// rendered by the Mermaid runtime after mount).
const renderer = new marked.Renderer();
renderer.code = ({ text, lang }: { text: string; lang?: string }) => {
  const language = (lang || "").split(/\s+/)[0] || "";
  if (language === "mermaid") {
    return `<div class="mermaid">${text.replace(/</g, "&lt;")}</div>`;
  }
  if (language && hljs.getLanguage(language)) {
    const highlighted = hljs.highlight(text, { language }).value;
    return `<pre><code class="hljs language-${language}">${highlighted}</code></pre>`;
  }
  const escaped = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return `<pre><code class="hljs">${escaped}</code></pre>`;
};
marked.setOptions({ renderer, breaks: true, gfm: true });

function renderMath(source: string): string {
  // Block math first, then inline math (best-effort; code fences are handled
  // by the pre-pass below because they are stripped first).
  const withoutFences = source.replace(/```[\s\S]*?```/g, (fence) => fence.replace(/\$/g, "&#36;"));
  const block = withoutFences.replace(/\$\$([\s\S]+?)\$\$/g, (_match, tex: string) =>
    katex.renderToString(tex.trim(), { displayMode: true, throwOnError: false }),
  );
  const inline = block.replace(/(^|[^$])\$([^$\n]+?)\$(?!\$)/g, (_match, prefix: string, tex: string) =>
    `${prefix}${katex.renderToString(tex.trim(), { displayMode: false, throwOnError: false })}`,
  );
  return inline;
}

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
    const math = renderMath(markdown);
    container.innerHTML = marked.parse(math) as string;
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