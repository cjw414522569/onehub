//! i18n (T053): the app UI supports Simplified Chinese (zh-CN), Traditional
//! Chinese (zh-TW) and English (en). The backend persists the chosen language
//! and validates the resource dictionaries; the frontend applies the
//! dictionaries to key chrome strings.

use crate::store::Store;
use serde_json::{json, Value};

/// Supported UI languages.
pub const SUPPORTED_LANGUAGES: &[(&str, &str)] = &[
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文"),
    ("en", "English"),
];

/// The settings key that stores the UI language.
pub const LANGUAGE_SETTING_KEY: &str = "ui.language";

/// The default language.
pub const DEFAULT_LANGUAGE: &str = "zh-CN";

/// Returns the supported language list (i18n_languages).
pub fn i18n_languages() -> Value {
    let languages: Vec<Value> = SUPPORTED_LANGUAGES
        .iter()
        .map(|(code, label)| json!({ "code": code, "label": label }))
        .collect();
    json!(languages)
}

/// Sets the UI language (i18n_set_language).
pub fn i18n_set_language(store: &mut Store, language: &str) -> Result<Value, String> {
    if !SUPPORTED_LANGUAGES
        .iter()
        .any(|(code, _)| *code == language)
    {
        return Err(format!("不支持的语言：{language}"));
    }
    store
        .put_app_setting(LANGUAGE_SETTING_KEY, &json!(language))
        .map_err(|e| e.to_string())?;
    Ok(json!({ "language": language }))
}

/// Reads the UI language (i18n_get_language).
pub fn i18n_get_language(store: &Store) -> Value {
    let language = store
        .get_app_setting(LANGUAGE_SETTING_KEY)
        .ok()
        .flatten()
        .and_then(|value| value.as_str().map(|s| s.to_string()))
        .filter(|language| SUPPORTED_LANGUAGES.iter().any(|(code, _)| code == language))
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string());
    json!({ "language": language })
}

/// Validates the resource dictionary for a language (i18n_validate).
/// Returns the number of keys; unknown languages are rejected.
pub fn i18n_validate(language: &str) -> Result<Value, String> {
    let resources = dictionary(language).ok_or_else(|| format!("不支持的语言：{language}"))?;
    Ok(json!({
        "language": language,
        "keys": resources.as_object().map(|o| o.len()).unwrap_or(0),
        "valid": true,
    }))
}

/// Returns the full translation dictionary for a language.
pub fn dictionary(language: &str) -> Option<Value> {
    let resources: serde_json::Map<String, Value> = match language {
        "zh-CN" => zh_cn()
            .into_iter()
            .map(|(key, value)| (key.to_string(), json!(value)))
            .collect(),
        "zh-TW" => zh_tw()
            .into_iter()
            .map(|(key, value)| (key, json!(value)))
            .collect(),
        "en" => en()
            .into_iter()
            .map(|(key, value)| (key.to_string(), json!(value)))
            .collect(),
        _ => return None,
    };
    Some(Value::Object(resources))
}

fn zh_cn() -> Vec<(&'static str, &'static str)> {
    vec![
        ("app.new_connection", "新建连接"),
        ("app.refresh", "刷新连接并探测延迟"),
        ("app.recent", "最近"),
        ("app.all", "全部"),
        ("app.favorites", "收藏"),
        ("app.settings", "设置"),
        ("app.connection", "连接"),
        ("app.database", "数据库工作台"),
        ("app.redis", "Redis 控制台"),
        ("app.mongo", "MongoDB 控制台"),
        ("app.notes", "Markdown 笔记"),
        ("app.broadcast", "终端广播"),
        ("app.copy_across", "跨服务器复制"),
        ("app.agent", "Agent Hub"),
        ("app.git", "Git 分支与 Diff"),
        ("app.acp", "ACP 外部 Agent"),
        ("app.extensions", "扩展市场"),
        ("app.terminal", "终端"),
        ("app.search", "搜索名称、地址、备注"),
        ("app.system", "系统"),
        ("app.last_connected", "最后连接"),
        ("app.latency", "延迟"),
        ("app.name", "名称"),
        ("app.notes_short", "备注"),
        ("app.actions", "操作"),
    ]
}

fn zh_tw() -> Vec<(String, String)> {
    zh_cn()
        .into_iter()
        .map(|(key, value)| (key.to_string(), to_traditional(value)))
        .collect()
}

fn en() -> Vec<(&'static str, &'static str)> {
    vec![
        ("app.new_connection", "New Connection"),
        ("app.refresh", "Refresh connections and probe latency"),
        ("app.recent", "Recent"),
        ("app.all", "All"),
        ("app.favorites", "Favorites"),
        ("app.settings", "Settings"),
        ("app.connection", "Connections"),
        ("app.database", "Database Workbench"),
        ("app.redis", "Redis Console"),
        ("app.mongo", "MongoDB Console"),
        ("app.notes", "Markdown Notes"),
        ("app.broadcast", "Terminal Broadcast"),
        ("app.copy_across", "Cross-server Copy"),
        ("app.agent", "Agent Hub"),
        ("app.git", "Git Branches & Diff"),
        ("app.acp", "ACP External Agent"),
        ("app.extensions", "Extension Marketplace"),
        ("app.terminal", "Terminal"),
        ("app.search", "Search name, address, notes"),
        ("app.system", "System"),
        ("app.last_connected", "Last connected"),
        ("app.latency", "Latency"),
        ("app.name", "Name"),
        ("app.notes_short", "Notes"),
        ("app.actions", "Actions"),
    ]
}

/// A tiny zh-CN -> zh-TW converter for the curated chrome strings (T053).
fn to_traditional(text: &str) -> String {
    const MAP: &[(&str, &str)] = &[
        ("连接", "連線"),
        ("新建", "新建"),
        ("刷新", "重新整理"),
        ("探测", "探測"),
        ("延迟", "延遲"),
        ("最近", "最近"),
        ("全部", "全部"),
        ("收藏", "收藏"),
        ("设置", "設定"),
        ("数据库", "資料庫"),
        ("工作台", "工作臺"),
        ("控制台", "控制臺"),
        ("笔记", "筆記"),
        ("终端", "終端"),
        ("广播", "廣播"),
        ("跨服务器", "跨伺服器"),
        ("复制", "複製"),
        ("扩展", "擴充"),
        ("市场", "市集"),
        ("搜索", "搜尋"),
        ("名称", "名稱"),
        ("地址", "地址"),
        ("备注", "備註"),
        ("操作", "操作"),
        ("系统", "系統"),
        ("最后", "最後"),
        ("分支", "分支"),
        ("与", "與"),
        ("外部", "外部"),
        ("打开", "開啟"),
        ("关闭", "關閉"),
        ("保存", "儲存"),
        ("删除", "刪除"),
        ("编辑", "編輯"),
        ("启动", "啟動"),
        ("停止", "停止"),
        ("加载", "載入"),
        ("导出", "匯出"),
        ("导入", "匯入"),
        ("图表", "圖表"),
        ("渲染", "渲染"),
        ("渲染器", "渲染器"),
    ];
    let mut out = text.to_string();
    for (from, to) in MAP {
        out = out.replace(from, to);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_roundtrip_and_validation() {
        let mut store = Store::open_in_memory().expect("store");
        assert_eq!(i18n_get_language(&store)["language"], DEFAULT_LANGUAGE);
        i18n_set_language(&mut store, "zh-TW").expect("set");
        assert_eq!(i18n_get_language(&store)["language"], "zh-TW");
        i18n_set_language(&mut store, "en").expect("set en");
        assert_eq!(i18n_get_language(&store)["language"], "en");
        let err = i18n_set_language(&mut store, "fr").expect_err("unsupported");
        assert!(err.contains("不支持"), "got {err:?}");
    }

    #[test]
    fn dictionaries_are_complete_and_traditional_works() {
        for (code, _) in SUPPORTED_LANGUAGES {
            let resources = dictionary(code).expect("dictionary");
            let keys = resources.as_object().expect("object");
            assert!(keys.len() >= 24, "{code} keys = {}", keys.len());
        }
        let zh_tw = dictionary("zh-TW").expect("zh-TW");
        assert_eq!(zh_tw["app.connection"], "連線");
        assert_eq!(zh_tw["app.settings"], "設定");
        let en = dictionary("en").expect("en");
        assert_eq!(en["app.connection"], "Connections");
    }

    #[test]
    fn validate_rejects_unknown_language() {
        let err = i18n_validate("fr").expect_err("unknown");
        assert!(err.contains("不支持"), "got {err:?}");
        let ok = i18n_validate("zh-TW").expect("ok");
        assert_eq!(ok["valid"], true);
    }
}
