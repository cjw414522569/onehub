import { marked } from "marked";
import hljs from "highlight.js";
import katex from "katex";

// Shared markdown -> HTML pipeline (marked + highlight.js + KaTeX). Mermaid
// diagrams and relative-media resolution are handled by the preview/export
// consumers so the renderer stays pure and deterministic.
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
  const withoutFences = source.replace(/```[\s\S]*?```/g, (fence) => fence.replace(/\$/g, "&#36;"));
  const block = withoutFences.replace(/\$\$([\s\S]+?)\$\$/g, (_match, tex: string) =>
    katex.renderToString(tex.trim(), { displayMode: true, throwOnError: false }),
  );
  return block.replace(/(^|[^$])\$([^$\n]+?)\$(?!\$)/g, (_match, prefix: string, tex: string) =>
    `${prefix}${katex.renderToString(tex.trim(), { displayMode: false, throwOnError: false })}`,
  );
}

export function renderMarkdown(source: string): string {
  return marked.parse(renderMath(source)) as string;
}

export function markdownToPlainText(source: string): string {
  return source
    .replace(/```[\s\S]*?```/g, "")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/[*_`~>]/g, "")
    .replace(/\$\$[\s\S]*?\$\$/g, "")
    .replace(/\$[^$\n]+\$/g, "")
    .replace(/\s*\n\s*\n/g, "\n");
}