//! Machine-readable JSON output for the CLI (T145).
//!
//! Every JSON payload carries `schema_version` (1) so scripts can pin
//! against a stable schema. The surface is small and hand-built (no serde
//! dependency) and deterministic.

/// The JSON output schema version.
pub const JSON_SCHEMA_VERSION: u32 = 1;

/// Escapes a string for JSON (quotes, backslashes, control chars).
pub fn escape_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

/// The version payload.
pub fn version_json(tool: &str, version: &str) -> String {
    format!(
        "{{\"schema_version\":{JSON_SCHEMA_VERSION},\"tool\":\"{}\",\"version\":\"{}\"}}",
        escape_json(tool),
        escape_json(version)
    )
}

/// The `config --check` payload.
pub fn config_check_json(ok: bool, hosts: Option<usize>, error: Option<&str>) -> String {
    match (ok, hosts, error) {
        (true, Some(hosts), _) => {
            format!("{{\"schema_version\":{JSON_SCHEMA_VERSION},\"ok\":true,\"hosts\":{hosts}}}")
        }
        (false, _, Some(error)) => format!(
            "{{\"schema_version\":{JSON_SCHEMA_VERSION},\"ok\":false,\"error\":\"{}\"}}",
            escape_json(error)
        ),
        _ => format!("{{\"schema_version\":{JSON_SCHEMA_VERSION},\"ok\":false}}"),
    }
}

/// A stable error payload (exit-code contract preserved in JSON).
pub fn error_json(code: i32, message: &str) -> String {
    format!(
        "{{\"schema_version\":{JSON_SCHEMA_VERSION},\"ok\":false,\"exit_code\":{code},\"error\":\"{}\"}}",
        escape_json(message)
    )
}

#[cfg(test)]
mod tests {
    use super::{config_check_json, error_json, escape_json, version_json, JSON_SCHEMA_VERSION};

    #[test]
    fn version_json_is_versioned() {
        let json = version_json("ssh-cli", "0.1.0");
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"tool\":\"ssh-cli\""));
        assert!(json.contains("\"version\":\"0.1.0\""));
    }

    #[test]
    fn config_check_json_is_versioned() {
        assert_eq!(
            config_check_json(true, Some(3), None),
            format!("{{\"schema_version\":{JSON_SCHEMA_VERSION},\"ok\":true,\"hosts\":3}}")
        );
        assert_eq!(
            config_check_json(false, None, Some("bad")),
            format!("{{\"schema_version\":{JSON_SCHEMA_VERSION},\"ok\":false,\"error\":\"bad\"}}")
        );
    }

    #[test]
    fn error_json_keeps_exit_code() {
        let json = error_json(4, "connection error");
        assert!(json.contains("\"exit_code\":4"));
        assert!(json.contains("\"error\":\"connection error\""));
    }

    #[test]
    fn escape_json_handles_quotes_and_controls() {
        assert_eq!(escape_json("a\"b\\c\n"), "a\\\"b\\\\c\\n");
        assert_eq!(escape_json("\u{0001}"), "\\u0001");
    }
}
