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

// ---- WASM extension runtime (T052) ----

use std::collections::HashMap;
use std::sync::Mutex;

/// A loaded WASM extension instance held by the sandboxed runtime.
struct WasmExtension {
    instance: wasmi::Instance,
    store: wasmi::Store<()>,
    exports: Vec<String>,
}

static WASM_REGISTRY: Mutex<Option<HashMap<String, WasmExtension>>> = Mutex::new(None);

fn wasm_registry() -> &'static Mutex<Option<HashMap<String, WasmExtension>>> {
    &WASM_REGISTRY
}

fn wasm_new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("wasm-{nanos:x}")
}

/// Loads a WASM module (base64) into the sandboxed runtime
/// (ext_wasm_load). Only the restricted `host.log` host function is exposed;
/// modules importing anything else fail to instantiate.
pub fn ext_wasm_load(wasm_base64: &str) -> Result<Value, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(wasm_base64)
        .map_err(|e| format!("WASM base64 解码失败：{e}"))?;
    let engine = wasmi::Engine::default();
    let module = wasmi::Module::new(&engine, &bytes).map_err(|e| format!("WASM 解析失败：{e}"))?;
    let mut store = wasmi::Store::new(&engine, ());
    let mut linker = wasmi::Linker::new(&engine);
    // Restricted API sandbox: only host.log is available to extensions.
    linker
        .func_wrap("host", "log", |value: i32| {
            eprintln!("[wasm-host] log {value}");
        })
        .map_err(|e| format!("宿主函数注册失败：{e}"))?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| format!("WASM 实例化失败（受限 API 沙箱）：{e}"))?;
    let exports: Vec<String> = module
        .exports()
        .map(|export| export.name().to_string())
        .collect();
    let id = wasm_new_id();
    wasm_registry()
        .lock()
        .expect("wasm registry lock")
        .get_or_insert_with(HashMap::new)
        .insert(
            id.clone(),
            WasmExtension {
                instance,
                store,
                exports: exports.clone(),
            },
        );
    Ok(json!({ "id": id, "exports": exports }))
}

/// Calls an exported WASM function with integer arguments (ext_wasm_call).
pub fn ext_wasm_call(handle: &str, function: &str, args: Vec<i64>) -> Result<Value, String> {
    let mut guard = wasm_registry().lock().expect("wasm registry lock");
    let ext = guard
        .as_mut()
        .and_then(|registry| registry.get_mut(handle))
        .ok_or_else(|| "WASM 扩展不存在或已卸载。".to_string())?;
    let func = ext
        .instance
        .get_func(&mut ext.store, function)
        .ok_or_else(|| format!("导出函数不存在：{function}"))?;
    let ty = func.ty(&ext.store);
    let inputs: Vec<wasmi::Val> = args
        .iter()
        .enumerate()
        .map(|(index, &arg)| {
            let param_ty = ty
                .params()
                .get(index)
                .copied()
                .unwrap_or(wasmi::ValType::I32);
            match param_ty {
                wasmi::ValType::I64 => wasmi::Val::I64(arg),
                _ => wasmi::Val::I32(arg as i32),
            }
        })
        .collect();
    let mut outputs = vec![wasmi::Val::I32(0); ty.results().len()];
    func.call(&mut ext.store, &inputs, &mut outputs)
        .map_err(|e| format!("WASM 调用失败：{e}"))?;
    let results: Vec<Value> = outputs
        .into_iter()
        .map(|value| match value {
            wasmi::Val::I32(v) => json!(v),
            wasmi::Val::I64(v) => json!(v),
            _ => Value::Null,
        })
        .collect();
    Ok(json!({ "handle": handle, "function": function, "results": results }))
}

/// Unloads a WASM extension (ext_wasm_unload).
pub fn ext_wasm_unload(handle: &str) -> Result<Value, String> {
    let removed = wasm_registry()
        .lock()
        .expect("wasm registry lock")
        .as_mut()
        .and_then(|registry| registry.remove(handle))
        .is_some();
    Ok(json!({ "handle": handle, "unloaded": removed }))
}

/// Lists loaded WASM extensions (ext_wasm_list).
pub fn ext_wasm_list() -> Value {
    let guard = wasm_registry().lock().expect("wasm registry lock");
    let items: Vec<Value> = guard
        .as_ref()
        .map(|registry| {
            registry
                .iter()
                .map(|(id, ext)| json!({ "id": id, "exports": ext.exports }))
                .collect()
        })
        .unwrap_or_default();
    json!(items)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_runtime_load_call_unload() {
        // (module (func (export "add") (param i32 i32) (result i32)
        //   local.get 0 local.get 1 i32.add))
        const ADD_WASM: &[u8] = &[
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
            0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, // type
            0x03, 0x02, 0x01, 0x00, // function
            0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, // export "add"
            0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b, // code
        ];
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(ADD_WASM);
        let loaded = ext_wasm_load(&encoded).expect("load");
        let handle = loaded["id"].as_str().expect("handle").to_string();
        assert!(loaded["exports"]
            .as_array()
            .map(|a| a.len() == 1)
            .unwrap_or(false));
        let call = ext_wasm_call(&handle, "add", vec![2, 3]).expect("call");
        assert_eq!(call["results"][0], 5, "got {call:?}");
        assert!(!ext_wasm_list().as_array().unwrap().is_empty());
        let unloaded = ext_wasm_unload(&handle).expect("unload");
        assert_eq!(unloaded["unloaded"], true);
        assert!(ext_wasm_call(&handle, "add", vec![1, 1]).is_err());
        assert!(ext_wasm_unload(&handle).expect("unload again")["unloaded"] == false);
    }

    #[test]
    fn wasm_sandbox_rejects_unavailable_imports() {
        // (module (import "host" "danger" (func)) (export "f" (func 0)))
        const DANGER_WASM: &[u8] = &[
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
            0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type ()->()
            0x02, 0x0f, 0x01, 0x04, 0x68, 0x6f, 0x73, 0x74, 0x06, 0x64, 0x61, 0x6e, 0x67, 0x65,
            0x72, 0x00, 0x00, // import host.danger
            0x03, 0x02, 0x01, 0x00, // function 0
            0x07, 0x05, 0x01, 0x01, 0x66, 0x00, 0x00, // export f
            0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b, // code (empty body)
        ];
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(DANGER_WASM);
        let err = ext_wasm_load(&encoded).expect_err("restricted import rejected");
        assert!(
            err.contains("实例化失败") || err.contains("沙箱"),
            "got {err:?}"
        );
    }
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
