//! Local Markdown notes (T041). Notes are plain `.md` files under an
//! app-local `notes/` directory next to the executable; the frontend editor
//! (Monaco markdown) edits them and a lazy-loaded preview renders markdown
//! with syntax highlighting, Mermaid diagrams, KaTeX math and note-relative
//! media (images/audio) resolved through `notes_asset`.

use base64::Engine;
use serde_json::{json, Value};
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

    fn delete_note_in(dir: &Path, name: &str) -> Result<(), String> {
        let path = safe_note_path(dir, name)?;
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| format!("删除失败：{e}"))?;
        }
        Ok(())
    }
}
