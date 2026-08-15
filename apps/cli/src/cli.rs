//! CLI configuration, connect/exec, and exit-code contract (T143).
//!
//! [`CliConfig`] parses a host-alias configuration; [`Cli`] connects and
//! runs a command through an injectable [`CommandRunner`] (the SSH/gateway
//! backend is wired later, mirroring the startup flow's injectable design).
//! [`ExitCode`] provides stable, documented process exit codes so scripts
//! behave deterministically, and the output formatter keeps interactive and
//! non-interactive output strictly separated: non-interactive stdout carries
//! only the remote command's stdout (no ANSI, no progress), while status
//! lines go to stderr.

use std::collections::BTreeMap;
use std::io::IsTerminal;

/// Stable process exit codes for the CLI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// The command succeeded.
    Ok = 0,
    /// An unexpected internal error.
    Internal = 1,
    /// Usage error (bad arguments).
    Usage = 2,
    /// Configuration error (missing or invalid host config).
    Config = 3,
    /// Connection error (could not reach the host/gateway).
    Connect = 4,
    /// The remote command failed (non-zero remote exit).
    Remote = 5,
}

impl ExitCode {
    /// The numeric exit code.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Maps a remote exit status to an [`ExitCode`]: 0 -> [`ExitCode::Ok`],
    /// otherwise [`ExitCode::Remote`].
    pub fn from_remote(remote_exit: i32) -> ExitCode {
        if remote_exit == 0 {
            ExitCode::Ok
        } else {
            ExitCode::Remote
        }
    }
}

/// A configured host alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    /// The alias (section header).
    pub alias: String,
    /// Hostname or IP.
    pub host: String,
    /// Remote user.
    pub user: String,
    /// SSH port.
    pub port: u16,
    /// Optional gateway URL for Web/CLI-over-gateway connections.
    pub gateway: Option<String>,
}

/// Why configuration parsing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A section had no `host =` line.
    MissingHostname,
    /// A section had no `user =` line.
    MissingUser,
    /// A `port =` value was not a valid 1..=65535 number.
    InvalidPort,
}

/// The parsed CLI configuration: hosts keyed by alias.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliConfig {
    /// Hosts by alias.
    pub hosts: BTreeMap<String, HostConfig>,
}

impl CliConfig {
    /// Parses a `key = value` config with `[alias]` sections.
    ///
    /// ```text
    /// [dev]
    /// host = dev.example.com
    /// user = admin
    /// port = 22
    /// gateway = wss://gw.example.com
    /// ```
    pub fn parse(text: &str) -> Result<CliConfig, ConfigError> {
        let mut hosts = BTreeMap::new();
        let mut current: Option<HostConfig> = None;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                if let Some(host) = current.take() {
                    hosts.insert(host.alias.clone(), host);
                }
                current = Some(HostConfig {
                    alias: line[1..line.len() - 1].trim().to_owned(),
                    host: String::new(),
                    user: String::new(),
                    port: 22,
                    gateway: None,
                });
                continue;
            }
            let (key, value) = line.split_once('=').ok_or(ConfigError::MissingHostname)?;
            let host = current.as_mut().ok_or(ConfigError::MissingHostname)?;
            match key.trim() {
                "host" => host.host = value.trim().to_owned(),
                "user" => host.user = value.trim().to_owned(),
                "port" => {
                    let parsed: u32 = value.trim().parse().map_err(|_| ConfigError::InvalidPort)?;
                    if !(1..=65535).contains(&parsed) {
                        return Err(ConfigError::InvalidPort);
                    }
                    host.port = parsed as u16;
                }
                "gateway" => host.gateway = Some(value.trim().to_owned()),
                _ => {}
            }
        }
        if let Some(host) = current.take() {
            hosts.insert(host.alias.clone(), host);
        }
        for host in hosts.values() {
            if host.host.is_empty() {
                return Err(ConfigError::MissingHostname);
            }
            if host.user.is_empty() {
                return Err(ConfigError::MissingUser);
            }
        }
        Ok(CliConfig { hosts })
    }

    /// Looks up a host by alias.
    pub fn get(&self, alias: &str) -> Option<&HostConfig> {
        self.hosts.get(alias)
    }
}

/// Output mode: interactive TTY vs non-interactive script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// A human-facing terminal (status lines allowed on stderr).
    Interactive,
    /// A script/pipe: stdout carries only the remote command's stdout.
    NonInteractive,
}

/// The result of running a command on a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    /// Remote command stdout.
    pub stdout: String,
    /// Remote command stderr.
    pub stderr: String,
    /// The remote process exit status (None when the connection failed).
    pub remote_exit: Option<i32>,
}

/// The injectable connect/exec backend (the SSH/gateway adapter is wired
/// later; the mock keeps the contract deterministic).
pub trait CommandRunner {
    /// Connects to the host; returns an error message on failure.
    fn connect(&self, host: &HostConfig) -> Result<(), String>;
    /// Runs a command over the connected session.
    fn exec(&self, command: &str) -> ExecResult;
}

/// A deterministic mock runner for tests and the CLI contract.
#[derive(Debug, Clone)]
pub struct MockRunner {
    /// The host this runner is bound to.
    pub host: HostConfig,
    /// The canned stdout.
    pub output: String,
    /// The canned remote exit status.
    pub remote_exit: Option<i32>,
}

impl CommandRunner for MockRunner {
    fn connect(&self, host: &HostConfig) -> Result<(), String> {
        if host == &self.host {
            Ok(())
        } else {
            Err(format!("cannot connect to {}", host.alias))
        }
    }

    fn exec(&self, _command: &str) -> ExecResult {
        ExecResult {
            stdout: self.output.clone(),
            stderr: String::new(),
            remote_exit: self.remote_exit,
        }
    }
}

/// CLI orchestration: connect, exec, and output formatting.
pub struct Cli {
    /// The injected backend.
    pub runner: Box<dyn CommandRunner>,
    /// The output mode for this invocation.
    pub mode: OutputMode,
}

impl Cli {
    /// Connects and runs `command`, returning the raw result.
    pub fn run(&self, host: &HostConfig, command: &str) -> Result<ExecResult, String> {
        self.runner.connect(host)?;
        Ok(self.runner.exec(command))
    }

    /// Formats the stdout stream for the current mode.
    ///
    /// Non-interactive output is **strictly separated**: stdout carries only
    /// the remote command's stdout, with no ANSI escapes and no progress or
    /// status lines. Interactive mode may prefix a host/status banner on
    /// stderr but still keeps stdout free of control bytes.
    pub fn format_stdout(&self, result: &ExecResult) -> String {
        let _ = self.mode;
        result.stdout.clone()
    }

    /// Formats the stderr stream for the current mode: status lines never
    /// pollute stdout. Non-interactive mode emits only remote stderr.
    pub fn format_stderr(&self, result: &ExecResult, banner: &str) -> String {
        match self.mode {
            OutputMode::Interactive => {
                if banner.is_empty() {
                    result.stderr.clone()
                } else {
                    format!("{banner}\n{}", result.stderr)
                }
            }
            OutputMode::NonInteractive => result.stderr.clone(),
        }
    }
}

/// Whether the process stdout is a terminal (the interactive probe).
pub fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Maps an [`ExitCode`] to its numeric value (convenience for main).
pub fn exit_code(code: ExitCode) -> i32 {
    code.code()
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, CliConfig, ConfigError, ExecResult, ExitCode, HostConfig, MockRunner, OutputMode,
    };

    const CONFIG: &str = r#"
# ssh-cli hosts
[dev]
host = dev.example.com
user = admin
port = 22
gateway = wss://gw.example.com

[prod-a]
host = 10.0.0.5
user = root
"#;

    #[test]
    fn config_parses_hosts_and_defaults_port() {
        let config = CliConfig::parse(CONFIG).unwrap();
        assert_eq!(config.hosts.len(), 2);
        let dev = config.get("dev").unwrap();
        assert_eq!(dev.host, "dev.example.com");
        assert_eq!(dev.user, "admin");
        assert_eq!(dev.port, 22);
        assert_eq!(dev.gateway.as_deref(), Some("wss://gw.example.com"));
        let prod = config.get("prod-a").unwrap();
        assert_eq!(prod.port, 22);
        assert_eq!(prod.gateway, None);
    }

    #[test]
    fn config_missing_fields_are_rejected() {
        assert_eq!(
            CliConfig::parse("[x]\nuser = a\n").unwrap_err(),
            ConfigError::MissingHostname
        );
        assert_eq!(
            CliConfig::parse("[x]\nhost = h\n").unwrap_err(),
            ConfigError::MissingUser
        );
        assert_eq!(
            CliConfig::parse("[x]\nhost = h\nuser = u\nport = 99999\n").unwrap_err(),
            ConfigError::InvalidPort
        );
    }

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(ExitCode::Ok.code(), 0);
        assert_eq!(ExitCode::Internal.code(), 1);
        assert_eq!(ExitCode::Usage.code(), 2);
        assert_eq!(ExitCode::Config.code(), 3);
        assert_eq!(ExitCode::Connect.code(), 4);
        assert_eq!(ExitCode::Remote.code(), 5);
        assert_eq!(ExitCode::from_remote(0), ExitCode::Ok);
        assert_eq!(ExitCode::from_remote(1), ExitCode::Remote);
        assert_eq!(ExitCode::from_remote(127), ExitCode::Remote);
    }

    #[test]
    fn exec_maps_remote_exit_to_stable_code() {
        let config = CliConfig::parse(CONFIG).unwrap();
        let dev = config.get("dev").unwrap().clone();
        let cli = Cli {
            runner: Box::new(MockRunner {
                host: dev.clone(),
                output: "ok\n".to_owned(),
                remote_exit: Some(0),
            }),
            mode: OutputMode::NonInteractive,
        };
        let result = cli.run(&dev, "true").unwrap();
        assert_eq!(result.stdout, "ok\n");
        assert_eq!(
            ExitCode::from_remote(result.remote_exit.unwrap()),
            ExitCode::Ok
        );
    }

    #[test]
    fn connect_failure_is_reported() {
        let config = CliConfig::parse(CONFIG).unwrap();
        let dev = config.get("dev").unwrap().clone();
        let other = HostConfig {
            alias: "elsewhere".to_owned(),
            host: "other.example".to_owned(),
            user: "u".to_owned(),
            port: 22,
            gateway: None,
        };
        let cli = Cli {
            runner: Box::new(MockRunner {
                host: other,
                output: String::new(),
                remote_exit: None,
            }),
            mode: OutputMode::NonInteractive,
        };
        assert!(cli.run(&dev, "ls").is_err());
    }

    #[test]
    fn non_interactive_stdout_is_separated() {
        let config = CliConfig::parse(CONFIG).unwrap();
        let dev = config.get("dev").unwrap().clone();
        let result = ExecResult {
            stdout: "file1\nfile2\n".to_owned(),
            stderr: "warning: x\n".to_owned(),
            remote_exit: Some(0),
        };
        let cli = Cli {
            runner: Box::new(MockRunner {
                host: dev.clone(),
                output: String::new(),
                remote_exit: Some(0),
            }),
            mode: OutputMode::NonInteractive,
        };
        // Non-interactive stdout = exactly the remote stdout; no ANSI.
        assert_eq!(cli.format_stdout(&result), "file1\nfile2\n");
        assert!(!cli.format_stdout(&result).contains('\u{1b}'));
        // Status banner goes to stderr in interactive mode only.
        assert_eq!(cli.format_stderr(&result, "status"), "warning: x\n");
        let interactive = Cli {
            runner: Box::new(MockRunner {
                host: dev.clone(),
                output: String::new(),
                remote_exit: Some(0),
            }),
            mode: OutputMode::Interactive,
        };
        assert_eq!(
            interactive.format_stderr(&result, "status"),
            "status\nwarning: x\n"
        );
        // Interactive stdout still never carries ANSI or banners.
        assert_eq!(interactive.format_stdout(&result), "file1\nfile2\n");
    }
}
