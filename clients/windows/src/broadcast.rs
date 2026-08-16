//! Terminal broadcast input (T043). When broadcast mode is enabled, input
//! typed in any terminal tab is fanned out to every active terminal session
//! (SSH + local); an explicit `terminal_broadcast` command offers the same
//! fan-out without the mode flag (e.g. the UI broadcast toolbar button).

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// Broadcast mode flag (shared by the mode toggle and the fan-out path).
static BROADCAST_MODE: AtomicBool = AtomicBool::new(false);

/// True when broadcast mode is on (checked by terminal_write fan-out).
pub fn broadcast_enabled() -> bool {
    BROADCAST_MODE.load(Ordering::SeqCst)
}

/// Sets the broadcast mode flag (terminal_set_broadcast).
pub fn set_broadcast(enabled: bool) -> Value {
    BROADCAST_MODE.store(enabled, Ordering::SeqCst);
    json!({ "enabled": enabled })
}

/// Returns the current broadcast mode (terminal_broadcast_status).
pub fn broadcast_status() -> Value {
    json!({ "enabled": BROADCAST_MODE.load(Ordering::SeqCst) })
}

/// All active terminal session ids (SSH + local), sorted and deduplicated.
pub fn active_session_ids() -> Vec<String> {
    let mut ids = crate::ssh_terminal::active_session_ids();
    ids.extend(crate::local_sessions::active_local_session_ids());
    ids.sort();
    ids.dedup();
    ids
}

/// Writes `input` to every active terminal session except `exclude`
/// (terminal_broadcast). Never fails: per-session write results are returned
/// for auditability so one dead session does not block the fan-out.
pub fn broadcast_write(input: &str, exclude: &str) -> Value {
    let bytes = input.as_bytes().to_vec();
    let mut results = Vec::new();
    for session_id in active_session_ids() {
        if !exclude.is_empty() && session_id == exclude {
            continue;
        }
        let ok = crate::ssh_terminal::write(&session_id, &bytes)
            .or_else(|_| crate::local_sessions::local_write(&session_id, &bytes))
            .is_ok();
        results.push(json!({ "session_id": session_id, "ok": ok }));
    }
    json!({
        "input": input,
        "targets": results.len(),
        "results": results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_mode_toggle_and_status() {
        set_broadcast(true);
        assert_eq!(broadcast_status()["enabled"], true);
        assert!(broadcast_enabled());
        set_broadcast(false);
        assert_eq!(broadcast_status()["enabled"], false);
        assert!(!broadcast_enabled());
    }

    #[test]
    fn broadcast_write_with_no_sessions_is_graceful() {
        let result = broadcast_write("echo hi\n", "");
        assert_eq!(result["targets"], 0);
        assert_eq!(
            result["results"].as_array().map(|a| a.len()).unwrap_or(0),
            0
        );
    }

    #[test]
    fn broadcast_fans_out_to_active_local_sessions() {
        // A plain interactive cmd.exe stays alive so the write succeeds.
        let id = crate::local_sessions::open_local(
            "cmd.exe",
            &[],
            None,
            Some("req-broadcast".to_string()),
            80,
            24,
        )
        .expect("open local");
        let ids = active_session_ids();
        assert!(ids.contains(&id), "got {ids:?}");
        let result = broadcast_write("REM broadcast\n", "");
        // Other tests may keep sessions open in the shared registry, so only
        // require that our own session received the write.
        let targets = result["targets"].as_u64().unwrap_or(0);
        assert!(targets >= 1, "got {result:?}");
        let own_ok = result["results"]
            .as_array()
            .map(|arr| {
                arr.iter().any(|entry| {
                    entry["session_id"].as_str() == Some(id.as_str())
                        && entry["ok"].as_bool() == Some(true)
                })
            })
            .unwrap_or(false);
        assert!(own_ok, "got {result:?}");
        let _ = crate::local_sessions::close_session(&id);
        assert!(!active_session_ids().contains(&id));
    }
}
