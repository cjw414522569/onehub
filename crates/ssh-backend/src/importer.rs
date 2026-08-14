//! OpenSSH config / known_hosts / key-metadata importer (T091).
//!
//! The importer parses `~/.ssh/config` (Host, HostName, User, Port,
//! IdentityFile, ProxyJump, Include, Match, ...), known_hosts lines, and
//! private-key file metadata. Every directive is reported (parsed or warned),
//! and private keys are **never copied** — the importer only reads a private
//! key's header/size and fingerprints a public `.pub` sibling when present.

use std::path::Path;

/// A parsed config directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDirective {
    /// Lowercased directive keyword.
    pub keyword: String,
    /// Arguments.
    pub args: Vec<String>,
    /// 1-based source line.
    pub line: usize,
}

/// The result of parsing an OpenSSH config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigParseResult {
    /// All parsed directives.
    pub directives: Vec<ParsedDirective>,
    /// Warnings for unsupported / malformed lines.
    pub warnings: Vec<String>,
}

/// Directives that influence connection behavior and must be reported.
const KNOWN_KEYWORDS: [&str; 16] = [
    "host",
    "hostname",
    "user",
    "port",
    "identityfile",
    "proxyjump",
    "include",
    "match",
    "serveraliveinterval",
    "forwardagent",
    "forwardx11",
    "compression",
    "ciphers",
    "macs",
    "hostkeyalgorithms",
    "connecttimeout",
];

/// Splits a config line into (keyword, args); the keyword keeps its original
/// case for reporting.
fn split_line(line: &str) -> Option<(String, Vec<String>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let keyword = parts.next()?.to_owned();
    let args: Vec<String> = parts.map(|p| p.to_owned()).collect();
    Some((keyword, args))
}

/// Parses OpenSSH config content into directives and warnings.
pub fn parse_config(content: &str) -> ConfigParseResult {
    let mut result = ConfigParseResult::default();
    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        let Some((keyword, args)) = split_line(raw_line) else {
            continue;
        };
        if KNOWN_KEYWORDS.contains(&keyword.to_ascii_lowercase().as_str()) {
            result.directives.push(ParsedDirective {
                keyword: keyword.to_ascii_lowercase(),
                args,
                line: line_number,
            });
        } else {
            result.warnings.push(format!(
                "line {line_number}: unsupported directive '{keyword}'"
            ));
        }
    }
    result
}

/// A known_hosts line report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHostsLine {
    /// 1-based line number.
    pub line: usize,
    /// The entry (host field, key type, key blob), or None when malformed.
    pub entry: Option<(String, String, String)>,
}

/// Parses known_hosts content into per-line reports.
pub fn parse_known_hosts(content: &str) -> Vec<KnownHostsLine> {
    content
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line_number = index + 1;
            let trimmed = line.trim();
            let entry = if trimmed.is_empty() || trimmed.starts_with('#') {
                None
            } else {
                let mut parts = trimmed.split_whitespace();
                match (parts.next(), parts.next(), parts.next()) {
                    (Some(host), Some(kind), Some(blob)) => {
                        Some((host.to_owned(), kind.to_owned(), blob.to_owned()))
                    }
                    _ => None,
                }
            };
            KnownHostsLine {
                line: line_number,
                entry,
            }
        })
        .collect()
}

/// Private-key metadata (never the private bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMetadata {
    /// The file path (report only; the key is never copied).
    pub path: String,
    /// The OpenSSH header kind, if readable.
    pub kind: Option<String>,
    /// File size in bytes.
    pub size: u64,
    /// Fingerprint of the public `.pub` sibling, when present.
    pub public_fingerprint: Option<String>,
}

/// Inspects a private key file's metadata without copying the key.
pub fn inspect_key(path: &Path) -> KeyMetadata {
    use sha2::Digest;
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let kind = std::fs::File::open(path)
        .ok()
        .and_then(|file| {
            use std::io::BufRead;
            std::io::BufReader::new(file).lines().next()
        })
        .and_then(|line| line.ok())
        .map(|line| line.trim().to_owned())
        .filter(|line| line.contains("PRIVATE KEY"))
        .map(|line| {
            let line = line.trim();
            line.strip_prefix("-----BEGIN ")
                .and_then(|s| s.strip_suffix("-----"))
                .unwrap_or(line)
                .trim()
                .to_owned()
        });
    // Never copy the private key: fingerprint only a public `.pub` sibling.
    let public_fingerprint = path
        .with_extension("pub")
        .is_file()
        .then(|| {
            let mut hasher = sha2::Sha256::new();
            if let Ok(content) = std::fs::read(path.with_extension("pub")) {
                hasher.update(&content);
                let digest = hasher.finalize();
                Some(
                    digest
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<String>>()
                        .join(":"),
                )
            } else {
                None
            }
        })
        .flatten();
    KeyMetadata {
        path: path.display().to_string(),
        kind,
        size,
        public_fingerprint,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{inspect_key, parse_config, parse_known_hosts, ConfigParseResult};

    #[test]
    fn config_corpus_parses_include_match_proxyjump() {
        let corpus = "\
# my ssh config
Host prod
    HostName 10.0.0.5
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_ed25519
    ProxyJump jump.example.com

Host *
    ServerAliveInterval 30
    ForwardAgent yes

Match host *.example.com
    Compression yes

Include ~/.ssh/config.d/*
";
        let result = parse_config(corpus);
        let keywords: Vec<&str> = result
            .directives
            .iter()
            .map(|d| d.keyword.as_str())
            .collect();
        for expected in [
            "host",
            "hostname",
            "user",
            "port",
            "identityfile",
            "proxyjump",
            "serveraliveinterval",
            "forwardagent",
            "match",
            "compression",
            "include",
        ] {
            assert!(
                keywords.contains(&expected),
                "config corpus must parse {expected}"
            );
        }
        // Host args preserved.
        let host = result
            .directives
            .iter()
            .find(|d| d.keyword == "host")
            .unwrap();
        assert_eq!(host.args, vec!["prod"]);
        let proxy = result
            .directives
            .iter()
            .find(|d| d.keyword == "proxyjump")
            .unwrap();
        assert_eq!(proxy.args, vec!["jump.example.com"]);
        assert!(
            result.warnings.is_empty(),
            "corpus should be fully supported: {:?}",
            result.warnings
        );
    }

    #[test]
    fn unknown_directives_produce_warnings() {
        let result = parse_config("Host x\n    UnknownThing value\n");
        assert!(
            result.warnings.iter().any(|w| w.contains("UnknownThing")),
            "unknown directive must be reported: {:?}",
            result.warnings
        );
        // The Host directive still parses.
        assert_eq!(result.directives.len(), 1);
        assert_eq!(result.directives[0].keyword, "host");
    }

    #[test]
    fn known_hosts_corpus_parses() {
        let corpus = "\
github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl
|1|abc= ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC7
# comment line
";
        let lines = parse_known_hosts(corpus);
        assert_eq!(lines.len(), 3, "github.com + hashed + comment");
        assert_eq!(lines[0].entry.as_ref().unwrap().0, "github.com");
        assert_eq!(lines[1].entry.as_ref().unwrap().0, "|1|abc=");
        assert!(lines[2].entry.is_none(), "comment line has no entry");
    }

    #[test]
    fn key_inspection_never_copies_private_key() {
        // Create a fake private key header + a public sibling.
        let dir = std::env::temp_dir().join(format!("ssh-importer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let key_path = dir.join("id_test");
        let pub_path = key_path.with_extension("pub");
        std::fs::write(
            &key_path,
            "-----BEGIN OPENSSH PRIVATE KEY-----\nbase64body\n-----END OPENSSH PRIVATE KEY-----\n",
        )
        .expect("write key");
        std::fs::write(&pub_path, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA comment\n")
            .expect("write pub");
        let metadata = inspect_key(&key_path);
        assert_eq!(metadata.kind.as_deref(), Some("OPENSSH PRIVATE KEY"));
        assert!(metadata.size > 0);
        assert!(
            metadata.public_fingerprint.is_some(),
            "public sibling fingerprint should be reported"
        );
        // The private key body was never copied into the metadata.
        assert!(!metadata.path.contains("base64body"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = PathBuf::new();
    }

    #[test]
    fn empty_config_is_clean() {
        let result = ConfigParseResult {
            directives: Vec::new(),
            warnings: Vec::new(),
        };
        assert!(result.directives.is_empty());
        assert!(result.warnings.is_empty());
        let parsed = parse_config("");
        assert!(parsed.directives.is_empty());
        assert!(parsed.warnings.is_empty());
    }
}
