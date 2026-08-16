//! Extension marketplace + provider framework (T051). The marketplace lists
//! pluggable providers (database drivers, RDP/VNC viewers, document
//! renderers); install/uninstall persist markers in the local store so the
//! registry is lifecycle-tested end to end.

use crate::store::Store;
use serde_json::{json, Value};

/// The provider catalog offered by the marketplace.
fn provider_catalog() -> Vec<Value> {
    let db_providers = crate::db::provider_registry();
    let mut catalog: Vec<Value> = db_providers
        .into_iter()
        .map(|provider| {
            let engine = provider["engine"].as_str().unwrap_or("").to_string();
            json!({
                "id": format!("db-{engine}"),
                "name": format!("DB 驱动：{}", provider["label"].as_str().unwrap_or(&engine)),
                "category": "database",
                "provider": provider,
                "builtin": true,
            })
        })
        .collect();
    catalog.push(json!({
        "id": "provider-rdp",
        "name": "RDP 查看器",
        "category": "rdp",
        "provider": { "kind": "rdp", "available": true },
        "builtin": true,
    }));
    catalog.push(json!({
        "id": "provider-vnc",
        "name": "VNC 查看器",
        "category": "vnc",
        "provider": { "kind": "vnc", "available": true },
        "builtin": true,
    }));
    for format in ["markdown", "pdf", "docx"] {
        catalog.push(json!({
            "id": format!("renderer-{format}"),
            "name": format!("文档渲染器：{format}"),
            "category": "renderer",
            "provider": { "kind": "renderer", "format": format, "available": true },
            "builtin": true,
        }));
    }
    catalog
}

/// Returns installed extension ids (from the store).
fn installed_ids(store: &Store) -> Vec<String> {
    store
        .list_extensions()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Lists the marketplace with installed flags (ext_marketplace_list).
pub fn ext_marketplace_list(store: &Store) -> Result<Value, String> {
    let installed = installed_ids(store);
    let items: Vec<Value> = provider_catalog()
        .into_iter()
        .map(|mut provider| {
            let id = provider["id"].as_str().unwrap_or("").to_string();
            let is_installed = installed.contains(&id);
            provider["installed"] = json!(is_installed);
            provider
        })
        .collect();
    Ok(json!({ "providers": items, "installed_count": installed.len() }))
}

/// Installs a provider (ext_install). Built-in providers are always available;
/// install records the marker so the marketplace reflects the choice.
pub fn ext_install(store: &mut Store, id: &str) -> Result<Value, String> {
    let known = provider_catalog()
        .iter()
        .any(|provider| provider["id"] == id);
    if !known {
        return Err(format!("未知扩展：{id}"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    store
        .put_extension(id, &json!({ "id": id, "installed_at": now }))
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "installed": true }))
}

/// Uninstalls a provider (ext_uninstall).
pub fn ext_uninstall(store: &mut Store, id: &str) -> Result<Value, String> {
    let removed = store.delete_extension(id).map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "uninstalled": removed }))
}

/// Lists installed extensions (ext_list).
pub fn ext_list(store: &Store) -> Result<Value, String> {
    let installed = store.list_extensions().map_err(|e| e.to_string())?;
    Ok(json!(installed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_lifecycle_install_uninstall() {
        let mut store = Store::open_in_memory().expect("store");
        let marketplace = ext_marketplace_list(&store).expect("marketplace");
        let providers = marketplace["providers"].as_array().expect("providers");
        assert!(providers.len() >= 15, "got {}", providers.len());
        assert!(providers.iter().any(|p| p["category"] == "database"));
        assert!(providers.iter().any(|p| p["category"] == "rdp"));
        assert!(providers.iter().any(|p| p["category"] == "renderer"));

        let installed = ext_install(&mut store, "db-mysql").expect("install");
        assert_eq!(installed["installed"], true);
        let unknown = ext_install(&mut store, "nope").expect_err("unknown");
        assert!(unknown.contains("未知扩展"), "got {unknown:?}");

        let after = ext_marketplace_list(&store).expect("after");
        let mysql = after["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .find(|p| p["id"] == "db-mysql")
            .expect("mysql provider");
        assert_eq!(mysql["installed"], true);

        let listed = ext_list(&store).expect("list");
        assert!(listed.as_array().map(|a| !a.is_empty()).unwrap_or(false));

        let removed = ext_uninstall(&mut store, "db-mysql").expect("uninstall");
        assert_eq!(removed["uninstalled"], true);
        let final_state = ext_marketplace_list(&store).expect("final");
        let mysql2 = final_state["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .find(|p| p["id"] == "db-mysql")
            .expect("mysql provider");
        assert_eq!(mysql2["installed"], false);
    }
}
