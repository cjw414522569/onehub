//! Theme import + accent color + window transparency (T054). Themes are
//! normalized JSON (VS Code-ish: name/accent/background/foreground/terminal)
//! persisted in the local store; accent and window alpha are app settings
//! with strict validation. The Win32 shell applies the alpha at startup.

use crate::store::Store;
use serde_json::{json, Value};

/// Settings keys.
pub const THEME_ACTIVE_KEY: &str = "theme.active";
pub const THEME_ACCENT_KEY: &str = "theme.accent";
pub const WINDOW_ALPHA_KEY: &str = "window.alpha";

/// Validates a #RRGGBB color.
fn valid_color(color: &str) -> bool {
    let color = color.trim().trim_start_matches('#');
    color.len() == 6 && color.chars().all(|c| c.is_ascii_hexdigit())
}

/// Normalizes an imported theme JSON into the canonical shape.
pub fn theme_normalize(content: &str) -> Result<Value, String> {
    let raw: Value = serde_json::from_str(content).map_err(|e| format!("主题 JSON 无效：{e}"))?;
    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Imported Theme".to_string());
    let accent = raw
        .get("accent")
        .and_then(Value::as_str)
        .unwrap_or("#2374c6")
        .to_string();
    if !valid_color(&accent) {
        return Err(format!("强调色无效：{accent}"));
    }
    let background = raw
        .get("background")
        .and_then(Value::as_str)
        .unwrap_or("#ffffff")
        .to_string();
    let foreground = raw
        .get("foreground")
        .and_then(Value::as_str)
        .unwrap_or("#1f2328")
        .to_string();
    let terminal = raw.get("terminal").cloned().unwrap_or(json!({}));
    Ok(json!({
        "name": name,
        "accent": accent,
        "background": background,
        "foreground": foreground,
        "terminal": terminal,
    }))
}

fn new_id() -> String {
    format!(
        "theme-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// Imports a theme (theme_import) and returns the saved theme.
pub fn theme_import(store: &mut Store, content: &str) -> Result<Value, String> {
    let mut theme = theme_normalize(content)?;
    let id = new_id();
    theme["id"] = json!(id);
    store.put_theme(&id, &theme).map_err(|e| e.to_string())?;
    Ok(theme)
}

/// Built-in theme presets (theme_list).
fn builtin_themes() -> Vec<Value> {
    vec![
        json!({
            "id": "builtin-light",
            "name": "浅色（默认）",
            "accent": "#2374c6",
            "background": "#ffffff",
            "foreground": "#1f2328",
            "terminal": { "background": "#0f172a", "foreground": "#e5e7eb", "cursor": "#2374c6" },
        }),
        json!({
            "id": "builtin-dark",
            "name": "深色",
            "accent": "#38bdf8",
            "background": "#0f172a",
            "foreground": "#e5e7eb",
            "terminal": { "background": "#020617", "foreground": "#e5e7eb", "cursor": "#38bdf8" },
        }),
    ]
}

/// Lists all themes: built-ins + imported, with the active id (theme_list).
pub fn theme_list(store: &Store) -> Result<Value, String> {
    let mut themes = builtin_themes();
    let imported = store.list_themes().map_err(|e| e.to_string())?;
    themes.extend(imported);
    let active = store
        .get_app_setting(THEME_ACTIVE_KEY)
        .ok()
        .flatten()
        .and_then(|value| value.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "builtin-light".to_string());
    Ok(json!({ "themes": themes, "active": active }))
}

/// Sets the active theme (theme_apply).
pub fn theme_apply(store: &mut Store, id: &str) -> Result<Value, String> {
    if id.is_empty() {
        return Err("主题 id 为空。".to_string());
    }
    store
        .put_app_setting(THEME_ACTIVE_KEY, &json!(id))
        .map_err(|e| e.to_string())?;
    Ok(json!({ "active": id }))
}

/// Sets the accent color (theme_set_accent).
pub fn theme_set_accent(store: &mut Store, color: &str) -> Result<Value, String> {
    if !valid_color(color) {
        return Err(format!("强调色无效：{color}"));
    }
    store
        .put_app_setting(THEME_ACCENT_KEY, &json!(color))
        .map_err(|e| e.to_string())?;
    Ok(json!({ "accent": color }))
}

/// Reads the configured accent color (theme_get_accent).
pub fn theme_get_accent(store: &Store) -> Value {
    let accent = store
        .get_app_setting(THEME_ACCENT_KEY)
        .ok()
        .flatten()
        .and_then(|value| value.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "#2374c6".to_string());
    json!({ "accent": accent })
}

/// Sets the window transparency percentage 0..=100 (window_set_alpha).
pub fn window_set_alpha(store: &mut Store, alpha: u8) -> Result<Value, String> {
    if alpha > 100 {
        return Err("透明度必须在 0-100 之间。".to_string());
    }
    store
        .put_app_setting(WINDOW_ALPHA_KEY, &json!(alpha))
        .map_err(|e| e.to_string())?;
    Ok(json!({ "alpha": alpha }))
}

/// Reads the configured window alpha (window_get_alpha).
pub fn window_get_alpha(store: &Store) -> Value {
    let alpha = store
        .get_app_setting(WINDOW_ALPHA_KEY)
        .ok()
        .flatten()
        .and_then(|value| value.as_u64())
        .map(|value| value.min(100) as u8)
        .unwrap_or(100);
    json!({ "alpha": alpha })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_import_normalizes_and_persists() {
        let mut store = Store::open_in_memory().expect("store");
        let imported = theme_import(
            &mut store,
            r##"{ "name": "OneDark", "accent": "#61afef", "background": "#282c34", "foreground": "#abb2bf", "terminal": { "background": "#21252b" } }"##,
        )
        .expect("import");
        assert_eq!(imported["name"], "OneDark");
        assert_eq!(imported["accent"], "#61afef");
        let listed = theme_list(&store).expect("list");
        assert!(listed["themes"]
            .as_array()
            .map(|a| a.len() >= 3)
            .unwrap_or(false));
        let bad = theme_import(&mut store, r##"{ "name": "x", "accent": "red" }"##)
            .expect_err("invalid accent");
        assert!(bad.contains("强调色无效"), "got {bad:?}");
    }

    #[test]
    fn accent_and_alpha_validation() {
        let mut store = Store::open_in_memory().expect("store");
        theme_set_accent(&mut store, "#38bdf8").expect("accent");
        assert_eq!(theme_get_accent(&store)["accent"], "#38bdf8");
        let bad = theme_set_accent(&mut store, "not-a-color").expect_err("bad");
        assert!(bad.contains("强调色无效"), "got {bad:?}");

        window_set_alpha(&mut store, 70).expect("alpha");
        assert_eq!(window_get_alpha(&store)["alpha"], 70);
        let over = window_set_alpha(&mut store, 101).expect_err("over");
        assert!(over.contains("0-100"), "got {over:?}");
    }

    #[test]
    fn theme_apply_and_active() {
        let mut store = Store::open_in_memory().expect("store");
        theme_apply(&mut store, "builtin-dark").expect("apply");
        let listed = theme_list(&store).expect("list");
        assert_eq!(listed["active"], "builtin-dark");
    }
}
