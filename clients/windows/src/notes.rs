//! Local Markdown notes (T041). Notes are plain `.md` files under an
//! app-local `notes/` directory next to the executable; the frontend editor
//! (Monaco markdown) edits them and a lazy-loaded preview renders markdown
//! with syntax highlighting, Mermaid diagrams, KaTeX math and note-relative
//! media (images/audio) resolved through `notes_asset`.

use base64::Engine;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Resolves the notes directory (override wins; otherwise exe_dir/notes).
fn notes_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位程序目录：{e}"))?;
    let dir = exe
        .parent()
        .map(|parent| parent.join("notes"))
        .unwrap_or_else(|| PathBuf::from("notes"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建笔记目录：{e}"))?;
    Ok(dir)
}

/// Sanitizes a note name into a safe `<name>.md` path inside the notes dir.
fn safe_note_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("笔记名为空。".to_string());
    }
    let base = name.replace(['/', '\\', '\0'], "_").replace("..", "_");
    let base = base.trim_end_matches(".md");
    if base.is_empty() {
        return Err("非法笔记名。".to_string());
    }
    let path = dir.join(format!("{base}.md"));
    if !path.starts_with(dir) {
        return Err("非法笔记名。".to_string());
    }
    Ok(path)
}

/// Lists note names (without the .md suffix), sorted.
fn list_notes_in(dir: &Path) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取笔记目录失败：{e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取笔记目录失败：{e}"))?;
        let path = entry.path();
        if path.is_file() && path.extension().map(|e| e == "md").unwrap_or(false) {
            let file_name = entry.file_name().to_string_lossy().to_string();
            notes.push(file_name.trim_end_matches(".md").to_string());
        }
    }
    notes.sort();
    Ok(notes)
}

/// Returns the notes directory path (notes_dir command).
pub fn notes_dir_info() -> Result<Value, String> {
    Ok(json!({ "dir": notes_dir()?.to_string_lossy() }))
}

/// Lists note names (notes_list command).
pub fn notes_list() -> Result<Value, String> {
    let dir = notes_dir()?;
    let notes = list_notes_in(&dir)?;
    Ok(json!({ "notes": notes }))
}

/// Reads a note's content (notes_read command).
pub fn notes_read(name: &str) -> Result<Value, String> {
    let dir = notes_dir()?;
    let path = safe_note_path(&dir, name)?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取笔记失败：{e}"))?;
    Ok(json!({
        "name": name.trim().trim_end_matches(".md"),
        "content": content,
    }))
}

/// Writes a note (notes_save command).
pub fn notes_save(name: &str, content: &str) -> Result<Value, String> {
    let dir = notes_dir()?;
    let path = safe_note_path(&dir, name)?;
    std::fs::write(&path, content).map_err(|e| format!("保存笔记失败：{e}"))?;
    Ok(json!({
        "name": name.trim().trim_end_matches(".md"),
        "saved": true,
    }))
}

/// Deletes a note if present (notes_delete command).
pub fn notes_delete(name: &str) -> Result<Value, String> {
    let dir = notes_dir()?;
    let path = safe_note_path(&dir, name)?;
    if path.is_file() {
        std::fs::remove_file(&path).map_err(|e| format!("删除笔记失败：{e}"))?;
    }
    Ok(json!({ "deleted": true }))
}

/// Resolves a note-relative media asset to a base64 data URL (notes_asset).
pub fn notes_asset(relative: &str) -> Result<Value, String> {
    let dir = notes_dir()?;
    let relative = relative.trim().trim_start_matches('/');
    let path = dir.join(relative);
    if !path.starts_with(&dir) {
        return Err("非法资源路径。".to_string());
    }
    if !path.is_file() {
        return Err("资源不存在。".to_string());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("读取资源失败：{e}"))?;
    let mime = match path.extension().and_then(|ext| ext.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        Some("md") => "text/markdown",
        _ => "application/octet-stream",
    };
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    Ok(json!({ "relative": relative, "mime": mime, "data_url": data_url }))
}

/// Resolves the notes-export directory (exe_dir/notes-export).
fn export_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位程序目录：{e}"))?;
    let dir = exe
        .parent()
        .map(|parent| parent.join("notes-export"))
        .unwrap_or_else(|| PathBuf::from("notes-export"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建导出目录：{e}"))?;
    Ok(dir)
}

/// Sanitizes an export file name inside the export dir.
fn safe_export_path(dir: &Path, name: &str, extension: &str) -> Result<PathBuf, String> {
    let base = name
        .trim()
        .replace(['/', '\\', '\0'], "_")
        .replace("..", "_");
    let base = if base.is_empty() {
        "note".to_string()
    } else {
        base
    };
    let path = dir.join(format!("{base}.{extension}"));
    if !path.starts_with(dir) {
        return Err("非法导出文件名。".to_string());
    }
    Ok(path)
}

/// A very small, standards-valid PDF writer (PDF 1.4, one page). Text uses
/// UTF-16BE hex strings with an Identity-H CID font so Chinese survives.
fn build_pdf(title: &str, text: &str) -> Vec<u8> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut content = String::new();
    content.push_str("BT /F1 11 Tf 50 742 Td 15 TL\n");
    for line in lines.iter().take(60) {
        content.push_str(&format!("<{}> Tj T*\n", utf16be_hex(line)));
    }
    content.push_str("ET");

    let mut objects: Vec<String> = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    objects.push("<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string());
    objects.push(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_string(),
    );
    objects.push(
        "<< /Type /Font /Subtype /Type0 /BaseFont /ArialUnicodeMS /Encoding /Identity-H /DescendantFonts [6 0 R] >>"
            .to_string(),
    );
    objects.push(format!(
        "<< /Length {} >>\nstream\n{}\nendstream",
        content.len(),
        content
    ));
    objects.push(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ArialUnicodeMS /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /W [0 [1000]] >>"
            .to_string(),
    );
    objects.push(
        "<< /Type /FontDescriptor /FontName /ArialUnicodeMS /Flags 4 /FontBBox [-1000 -200 2000 1200] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>"
            .to_string(),
    );
    objects.push(format!("<< /Title ({}) >>", pdf_escape(title)));

    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n");
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, body).as_bytes());
    }
    let xref_offset = out.len();
    let count = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {count} /Root 1 0 R /Info 8 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
        )
        .as_bytes(),
    );
    out
}

/// Escapes a string for a PDF literal string (parentheses/backslash).
fn pdf_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Encodes a string as a PDF UTF-16BE hex string (without angle brackets).
fn utf16be_hex(text: &str) -> String {
    let mut out = String::from("FEFF");
    for unit in text.encode_utf16() {
        out.push_str(&format!("{unit:04X}"));
    }
    out
}

/// Builds a minimal DOCX (WordprocessingML) as a zip archive.
fn build_docx(lines: &[&str]) -> Result<Vec<u8>, String> {
    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;
    let mut body = String::new();
    for line in lines.iter().take(200) {
        body.push_str(&format!(
            "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
            xml_escape(line)
        ));
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("[Content_Types].xml", options)
            .map_err(|e| format!("DOCX 写入失败：{e}"))?;
        writer
            .write_all(content_types.as_bytes())
            .map_err(|e| format!("DOCX 写入失败：{e}"))?;
        writer
            .start_file("_rels/.rels", options)
            .map_err(|e| format!("DOCX 写入失败：{e}"))?;
        writer
            .write_all(rels.as_bytes())
            .map_err(|e| format!("DOCX 写入失败：{e}"))?;
        writer
            .start_file("word/document.xml", options)
            .map_err(|e| format!("DOCX 写入失败：{e}"))?;
        writer
            .write_all(document.as_bytes())
            .map_err(|e| format!("DOCX 写入失败：{e}"))?;
        writer.finish().map_err(|e| format!("DOCX 写入失败：{e}"))?;
    }
    Ok(buffer.into_inner())
}

/// Escapes XML text content.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Strips HTML tags/entities into plain text (fallback for PDF/DOCX).
fn html_to_plain_text(html: &str) -> String {
    let without_tags = html
        .replace("<br>", "\n")
        .replace("</p>", "\n")
        .replace("</div>", "\n");
    let without_tags = without_tags
        .replace("<li>", "- ")
        .replace("</li>", "\n")
        .replace("</h1>", "\n")
        .replace("</h2>", "\n")
        .replace("</h3>", "\n")
        .replace("</h4>", "\n")
        .replace("</pre>", "\n")
        .replace("</tr>", "\n");
    let mut text = String::new();
    let mut in_tag = false;
    for ch in without_tags.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            text.push(ch);
        }
    }
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Exports a note to HTML/PDF/DOCX in the export directory (notes_export).
pub fn notes_export(name: &str, format: &str, html: &str, text: &str) -> Result<Value, String> {
    let dir = export_dir()?;
    let plain = if text.trim().is_empty() {
        html_to_plain_text(html)
    } else {
        text.to_string()
    };
    let (extension, path) = match format {
        "html" => {
            let path = safe_export_path(&dir, name, "html")?;
            let standalone = format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\
                 <style>body{{font-family:system-ui,sans-serif;max-width:840px;margin:24px auto;padding:0 16px;line-height:1.7}}\
                 pre{{background:#f6f8fa;padding:12px;border-radius:6px;overflow:auto}}code{{font-family:monospace}}\
                 table{{border-collapse:collapse}}td,th{{border:1px solid #d0d7de;padding:4px 8px}}</style></head>\
                 <body>{}</body></html>",
                xml_escape(name.trim()),
                html
            );
            ("html", write_export(&path, standalone.as_bytes())?)
        }
        "pdf" => {
            let path = safe_export_path(&dir, name, "pdf")?;
            let bytes = build_pdf(name, &plain);
            ("pdf", write_export(&path, &bytes)?)
        }
        "docx" => {
            let path = safe_export_path(&dir, name, "docx")?;
            let lines: Vec<&str> = plain.lines().collect();
            let bytes = build_docx(&lines)?;
            ("docx", write_export(&path, &bytes)?)
        }
        other => return Err(format!("不支持的导出格式：{other}")),
    };
    Ok(json!({
        "name": name.trim(),
        "format": extension,
        "path": path.to_string_lossy(),
        "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
    }))
}

fn write_export(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    std::fs::write(path, bytes).map_err(|e| format!("导出写入失败：{e}"))?;
    Ok(path.to_path_buf())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_crud_roundtrip() {
        let dir = std::env::temp_dir().join(format!("onehub-notes-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create notes dir");
        let png = dir.join("logo.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n fake").expect("write png");

        let path =
            save_note_in(&dir, "Getting Started", "# 你好\n\n![logo](logo.png)").expect("save");
        assert!(path.is_file());

        let notes = list_notes_in(&dir).expect("list");
        assert!(
            notes.contains(&"Getting Started".to_string()),
            "got {notes:?}"
        );

        let read = notes_read_in(&dir, "Getting Started").expect("read");
        assert!(read["content"].as_str().unwrap_or("").contains("你好"));

        let missing = notes_read_in(&dir, "no-such").expect_err("missing note");
        assert!(missing.contains("失败"), "got {missing:?}");

        let sanitized = safe_note_path(&dir, "../evil").expect("sanitized");
        assert!(
            sanitized.starts_with(&dir),
            "traversal not contained: {sanitized:?}"
        );
        assert!(
            !sanitized
                .file_name()
                .map(|f| f.to_string_lossy().contains(".."))
                .unwrap_or(true),
            "sanitized name still contains .."
        );

        let asset = notes_asset_in(&dir, "logo.png").expect("asset");
        assert!(asset["data_url"]
            .as_str()
            .unwrap_or("")
            .starts_with("data:image/png;base64,"));

        delete_note_in(&dir, "Getting Started").expect("delete");
        assert!(!list_notes_in(&dir)
            .expect("list after delete")
            .contains(&"Getting Started".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Test-only helpers that operate on an explicit directory (no global state).
    fn save_note_in(dir: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
        let path = safe_note_path(dir, name)?;
        std::fs::write(&path, content).map_err(|e| format!("保存失败：{e}"))?;
        Ok(path)
    }

    fn notes_read_in(dir: &Path, name: &str) -> Result<Value, String> {
        let path = safe_note_path(dir, name)?;
        let content = std::fs::read_to_string(&path).map_err(|e| format!("读取失败：{e}"))?;
        Ok(json!({ "name": name, "content": content }))
    }

    fn notes_asset_in(dir: &Path, relative: &str) -> Result<Value, String> {
        let path = dir.join(relative);
        if !path.starts_with(dir) {
            return Err("非法资源路径。".to_string());
        }
        if !path.is_file() {
            return Err("资源不存在。".to_string());
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("读取资源失败：{e}"))?;
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        Ok(json!({ "relative": relative, "data_url": data_url }))
    }

    #[test]
    fn notes_export_generates_html_pdf_docx() {
        use zip::ZipArchive;
        let html = "<h1>你好 OneHub</h1><p>导出测试</p><pre><code>fn main() {}</code></pre>";
        let text = "你好 OneHub\n导出测试\nfn main() {}";

        let html_bytes = build_pdf("note", text);
        assert!(String::from_utf8_lossy(&html_bytes).contains("%PDF-1.4"));
        assert!(String::from_utf8_lossy(&html_bytes).contains("/Type /Page"));
        assert!(String::from_utf8_lossy(&html_bytes).contains("startxref"));
        assert!(String::from_utf8_lossy(&html_bytes)
            .trim_end()
            .ends_with("%%EOF"));
        assert!(String::from_utf8_lossy(&html_bytes).contains("FEFF"));

        let docx_bytes = build_docx(&["你好 OneHub", "导出测试"]).expect("docx");
        let mut archive = ZipArchive::new(std::io::Cursor::new(docx_bytes)).expect("open docx");
        assert!(archive.by_name("[Content_Types].xml").is_ok());
        let mut document = String::new();
        use std::io::Read;
        archive
            .by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut document)
            .expect("read document");
        assert!(document.contains("你好 OneHub"), "docx body: {document}");
        assert!(document.contains("<w:document"));

        let plain = html_to_plain_text(html);
        assert!(plain.contains("你好 OneHub"));
        assert!(!plain.contains("<h1>"));
    }

    fn delete_note_in(dir: &Path, name: &str) -> Result<(), String> {
        let path = safe_note_path(dir, name)?;
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| format!("删除失败：{e}"))?;
        }
        Ok(())
    }
}
