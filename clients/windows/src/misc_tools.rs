//! Known-host trust, local path metadata, Windows PTY info, and window
//! material helpers (mxterm parity T018).
//!
//! Known hosts persist in the local SQLite store; the PTY backend and
//! supported DWM window materials are detected from the real OS build number.
//! The DWM attribute call itself is unsafe and therefore applied by the shell
//! (main.rs), not in this forbid(unsafe_code) library.

use serde_json::{json, Value};

use crate::store::Store;

fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// known_host_trust: records/updates a trusted host key (mxterm
/// `known_host_trust`).
pub fn known_host_trust(store: &mut Store, host_key: &Value) -> Result<Value, String> {
    store
        .trust_known_host(host_key, &now_ts())
        .map_err(|e| e.to_string())
}

/// known_host_check: compares a presented host key against the trusted entry.
pub fn known_host_check(
    store: &Store,
    host: &str,
    port: u64,
    algorithm: &str,
    fingerprint: &str,
) -> Result<Value, String> {
    let entry = store
        .known_host_lookup(host, port, algorithm)
        .map_err(|e| e.to_string())?;
    match entry {
        Some(entry) => {
            let trusted = entry
                .get("fingerprint_sha256")
                .and_then(Value::as_str)
                .unwrap_or("");
            Ok(json!({
                "trusted": true,
                "match": trusted == fingerprint,
                "entry": entry,
            }))
        }
        None => Ok(json!({ "trusted": false, "match": false })),
    }
}

/// local_path_metadata: returns kind/name/path for an existing local path.
pub fn local_path_metadata(path: &str) -> Result<Value, String> {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return Err("路径不存在。".to_string());
    }
    let kind = if path.is_dir() {
        "directory"
    } else if path.is_file() {
        "file"
    } else {
        "other"
    };
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    Ok(json!({
        "kind": kind,
        "name": name,
        "path": path.to_string_lossy(),
    }))
}

/// get_windows_pty_info: reports the ConPTY/WinPTY backend from the real
/// Windows build number (ConPTY requires build >= 17763).
pub fn windows_pty_info() -> Value {
    #[cfg(windows)]
    {
        let build = windows_build_number();
        let backend = if build.map(|b| b >= 17763).unwrap_or(true) {
            "conpty"
        } else {
            "winpty"
        };
        json!({ "backend": backend, "build_number": build })
    }
    #[cfg(not(windows))]
    {
        Value::Null
    }
}

/// get_supported_window_materials: DWM backdrop materials available on this
/// build (0 auto always; mica/acrylic/tabbed on build >= 22523).
pub fn supported_window_materials() -> Value {
    let mut materials = vec![window_material_info(0)];
    #[cfg(windows)]
    {
        if let Some(build) = windows_build_number() {
            if build >= 22523 {
                materials.push(window_material_info(2));
                materials.push(window_material_info(3));
                materials.push(window_material_info(4));
            }
        }
    }
    json!(materials)
}

/// True when the OS supports the DWM system-backdrop attribute
/// (mica/acrylic/tabbed, Windows 11 build 22523+).
pub fn dwm_backdrop_supported() -> bool {
    #[cfg(windows)]
    {
        windows_build_number()
            .map(|build| build >= 22523)
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn window_material_info(material: i32) -> Value {
    let name = match material {
        0 => "auto",
        2 => "mica",
        3 => "acrylic",
        4 => "tabbed",
        _ => "auto",
    };
    json!({ "id": material, "name": name })
}

pub fn normalize_material(material: i32) -> Result<i32, String> {
    match material {
        0 | 2 | 3 | 4 => Ok(material),
        _ => Err(format!("不支持的窗口材质：{material}")),
    }
}

#[cfg(windows)]
fn windows_build_number() -> Option<u64> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "/v",
            "CurrentBuildNumber",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().find_map(|line| {
        if line.to_ascii_lowercase().contains("currentbuildnumber") {
            line.split_whitespace()
                .last()
                .and_then(|value| value.parse::<u64>().ok())
        } else {
            None
        }
    })
}

#[cfg(not(windows))]
fn windows_build_number() -> Option<u64> {
    None
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_path_metadata_file_and_directory() {
        let dir = std::env::temp_dir().join(format!("misc-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("sample.txt");
        std::fs::write(&file, b"hello").expect("write");
        let meta = local_path_metadata(file.to_str().expect("str")).expect("file meta");
        assert_eq!(meta["kind"], "file");
        assert_eq!(meta["name"], "sample.txt");
        let dir_meta = local_path_metadata(dir.to_str().expect("str")).expect("dir meta");
        assert_eq!(dir_meta["kind"], "directory");
        assert!(local_path_metadata("Z:/definitely/missing/path").is_err());
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn known_host_trust_and_check_roundtrip() {
        let dir = std::env::temp_dir().join(format!("misc-kh-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("kh.db");
        let mut store = Store::open(&db).expect("store");
        let host_key = json!({
            "host": "10.0.0.1",
            "port": 22,
            "key_algorithm": "ssh-ed25519",
            "fingerprint_sha256": "abc123",
            "public_key": "ssh-ed25519 AAAA...",
        });
        let trusted = known_host_trust(&mut store, &host_key).expect("trust");
        assert_eq!(trusted["host"], "10.0.0.1");
        assert_eq!(trusted["key_algorithm"], "ssh-ed25519");
        let check =
            known_host_check(&store, "10.0.0.1", 22, "ssh-ed25519", "abc123").expect("check");
        assert_eq!(check["trusted"], true);
        assert_eq!(check["match"], true);
        let mismatch =
            known_host_check(&store, "10.0.0.1", 22, "ssh-ed25519", "different").expect("mismatch");
        assert_eq!(mismatch["trusted"], true);
        assert_eq!(mismatch["match"], false);
        let unknown =
            known_host_check(&store, "10.0.0.9", 22, "ssh-ed25519", "abc123").expect("unknown");
        assert_eq!(unknown["trusted"], false);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn window_material_names_and_normalization() {
        assert_eq!(window_material_info(0)["name"], "auto");
        assert_eq!(window_material_info(2)["name"], "mica");
        assert_eq!(window_material_info(3)["name"], "acrylic");
        assert_eq!(window_material_info(4)["name"], "tabbed");
        assert_eq!(normalize_material(2).expect("2"), 2);
        assert!(normalize_material(1).is_err());
    }

    #[test]
    fn pty_info_shape_on_windows() {
        let info = windows_pty_info();
        if cfg!(windows) {
            assert!(info["backend"] == "conpty" || info["backend"] == "winpty");
            assert!(info["build_number"].is_number() || info["build_number"].is_null());
        } else {
            assert!(info.is_null());
        }
    }
}
