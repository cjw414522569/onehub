//! Git branch + side-by-side diff support (T048). Runs the real `git` CLI for
//! branch/status/diff/switch and parses unified diffs into structured hunks
//! that the UI renders as a side-by-side view.

use serde_json::{json, Value};
use std::process::Command;

/// Parses a unified diff into structured hunks. Pure and unit-testable.
pub fn parse_unified_diff(text: &str) -> Vec<Value> {
    let mut hunks = Vec::new();
    let mut current: Option<Vec<Value>> = None;
    let mut old_line = 0i64;
    let mut new_line = 0i64;
    let mut old_count = 0i64;
    let mut new_count = 0i64;

    for line in text.lines() {
        if let Some(header) = parse_hunk_header(line) {
            if let Some(lines) = current.take() {
                hunks.push(json!({
                    "old_start": old_count,
                    "old_lines": old_count,
                    "new_start": new_count,
                    "new_lines": new_count,
                    "lines": lines,
                }));
            }
            old_line = header.0;
            new_line = header.1;
            old_count = header.0;
            new_count = header.1;
            current = Some(Vec::new());
            continue;
        }
        if let Some(lines) = current.as_mut() {
            let (kind, old, new, content) = classify_diff_line(line, old_line, new_line);
            lines.push(json!({
                "type": kind,
                "old_line": old,
                "new_line": new,
                "text": content,
            }));
            if old > 0 {
                old_line += 1;
            }
            if new > 0 {
                new_line += 1;
            }
        }
    }
    if let Some(lines) = current.take() {
        hunks.push(json!({
            "old_start": old_count,
            "old_lines": old_count,
            "new_start": new_count,
            "new_lines": new_count,
            "lines": lines,
        }));
    }
    hunks
}

fn parse_hunk_header(line: &str) -> Option<(i64, i64)> {
    let line = line.trim_end_matches('\r');
    if !line.starts_with("@@") {
        return None;
    }
    let rest = line.trim_start_matches("@@").trim_end_matches("@@").trim();
    let mut parts = rest.split_whitespace();
    let old = parts
        .next()?
        .trim_start_matches('-')
        .split(',')
        .next()?
        .parse::<i64>()
        .ok()?;
    let new = parts
        .next()?
        .trim_start_matches('+')
        .split(',')
        .next()?
        .parse::<i64>()
        .ok()?;
    Some((old, new))
}

fn classify_diff_line(
    line: &str,
    old_line: i64,
    new_line: i64,
) -> (&'static str, i64, i64, String) {
    let line = line.trim_end_matches('\r');
    if let Some(content) = line.strip_prefix('+') {
        ("add", 0, new_line, content.to_string())
    } else if let Some(content) = line.strip_prefix('-') {
        ("del", old_line, 0, content.to_string())
    } else if line.starts_with("\\") {
        ("meta", 0, 0, line.to_string())
    } else {
        let content = line.strip_prefix(' ').unwrap_or(line);
        ("context", old_line, new_line, content.to_string())
    }
}

/// Runs `git` in a repo directory and returns stdout on success.
fn run_git(repo: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git 执行失败：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} 失败：{}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Lists branches (git_branches).
pub fn git_branches(repo: &str) -> Result<Value, String> {
    let text = run_git(repo, &["branch", "--list"])?;
    let mut branches = Vec::new();
    let mut current = "";
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let (marker, name) = if let Some(name) = line.strip_prefix("* ") {
            ("*", name)
        } else if let Some(name) = line.strip_prefix("  ") {
            ("", name)
        } else {
            ("", line)
        };
        if !name.trim().is_empty() {
            if marker == "*" {
                current = name.trim();
            }
            branches.push(json!({ "name": name.trim(), "current": marker == "*" }));
        }
    }
    Ok(json!({ "branches": branches, "current": current }))
}

/// Lists working-tree changes (git_status).
pub fn git_status(repo: &str) -> Result<Value, String> {
    let text = run_git(repo, &["status", "--porcelain"])?;
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let code = &line[..2];
        let path = line[3..]
            .trim_start_matches('"')
            .trim_end_matches('"')
            .to_string();
        entries.push(json!({ "path": path, "status": code }));
    }
    Ok(json!({ "entries": entries }))
}

/// Returns the unified diff for one file (git_diff).
pub fn git_diff(repo: &str, file: &str) -> Result<Value, String> {
    let text = run_git(repo, &["diff", "--", file])?;
    let hunks = parse_unified_diff(&text);
    Ok(json!({ "file": file, "hunks": hunks, "raw": text }))
}

/// Switches to a branch (git_switch).
pub fn git_switch(repo: &str, branch: &str) -> Result<Value, String> {
    let text = run_git(repo, &["switch", branch])?;
    Ok(json!({ "branch": branch, "output": text.trim() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_diff_parses_hunks_and_lines() {
        let fixture = "@@ -1,4 +1,4 @@\n one\n-two\n+two changed\n three\n four\n@@ -10,2 +10,3 @@\n context\n+a\n+b\n";
        let hunks = parse_unified_diff(fixture);
        assert_eq!(hunks.len(), 2, "got {hunks:?}");
        let first = &hunks[0];
        assert_eq!(first["old_start"], 1);
        assert_eq!(first["new_start"], 1);
        let lines = first["lines"].as_array().expect("lines");
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1]["type"], "del");
        assert_eq!(lines[1]["old_line"], 2);
        assert_eq!(lines[2]["type"], "add");
        assert_eq!(lines[2]["new_line"], 2);
        assert_eq!(lines[2]["text"], "two changed");
        assert_eq!(lines[0]["type"], "context");
    }

    #[test]
    fn empty_diff_has_no_hunks() {
        assert!(parse_unified_diff("").is_empty());
        assert!(parse_unified_diff("no diff markers here").is_empty());
    }

    #[test]
    fn git_commands_graceful_without_repo() {
        let err = git_branches("C:/definitely-not-a-repo-xyz").expect_err("no repo");
        assert!(err.contains("失败") || err.contains("git"), "got {err:?}");
    }
}
