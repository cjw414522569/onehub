use std::collections::VecDeque;
use std::sync::Mutex;

use core_domain::command::EnvVar;
use core_domain::error::DomainError;

/// PTY request configuration (RFC 4254 §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyConfig {
    /// Terminal type (e.g. `xterm-256color`).
    pub term: String,
    /// Columns.
    pub columns: u16,
    /// Rows.
    pub rows: u16,
    /// Pixel width (may be 0).
    pub pixel_width: u16,
    /// Pixel height (may be 0).
    pub pixel_height: u16,
}

impl PtyConfig {
    /// Creates a PTY config, rejecting an empty TERM.
    pub fn new(
        term: impl Into<String>,
        columns: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<Self, DomainError> {
        let term = term.into();
        if term.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        Ok(Self {
            term,
            columns,
            rows,
            pixel_width,
            pixel_height,
        })
    }
}

/// A single command for exec mode with an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecCommand {
    /// The command line.
    pub command: String,
    /// Environment variables.
    pub env: Vec<EnvVar>,
}

impl ExecCommand {
    /// Creates an exec command, rejecting an empty command.
    pub fn new(command: impl Into<String>, env: Vec<EnvVar>) -> Result<Self, DomainError> {
        let command = command.into();
        if command.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        Ok(Self { command, env })
    }
}

/// Exit status of a channel (RFC 4254 §6.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitStatus {
    /// Numeric exit code, if reported.
    pub code: Option<u32>,
    /// Signal name, if terminated by a signal.
    pub signal: Option<String>,
}

impl ExitStatus {
    /// Whether the channel exited successfully (code 0 and no signal).
    pub fn is_success(&self) -> bool {
        self.signal.is_none() && self.code == Some(0)
    }

    /// Exit status for a successful run.
    pub fn success() -> Self {
        Self {
            code: Some(0),
            signal: None,
        }
    }
}

/// A channel event: output data and/or an exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEvent {
    /// Output bytes (None when only the exit status is reported).
    pub data: Option<Vec<u8>>,
    /// Exit status (None for pure data events).
    pub exit: Option<ExitStatus>,
}

impl ChannelEvent {
    /// A data event.
    pub fn data(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            data: Some(bytes.into()),
            exit: None,
        }
    }

    /// An exit-status event.
    pub fn exit(status: ExitStatus) -> Self {
        Self {
            data: None,
            exit: Some(status),
        }
    }
}

/// Channel operation error (no secret context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelError {
    /// Protocol error.
    Protocol,
    /// The channel is closed.
    Closed,
    /// I/O failure.
    Io,
}

/// A session channel: PTY, shell, exec, env, window-size, exit status.
#[allow(async_fn_in_trait)]
pub trait SessionChannel: Send + Sync {
    /// Requests a PTY.
    async fn request_pty(&self, config: &PtyConfig) -> Result<(), ChannelError>;
    /// Starts an interactive shell.
    async fn start_shell(&self) -> Result<(), ChannelError>;
    /// Executes a single command.
    async fn exec(&self, command: &ExecCommand) -> Result<(), ChannelError>;
    /// Changes the window (terminal) size.
    async fn set_window_size(&self, columns: u16, rows: u16) -> Result<(), ChannelError>;
    /// Reads the next channel event.
    async fn read_event(&self) -> Result<ChannelEvent, ChannelError>;
    /// Closes the channel.
    async fn close(&self) -> Result<(), ChannelError>;
}

/// A scripted channel for integration tests.
///
/// Records the calls it receives and replays a scripted event sequence via
/// `read_event`, so the shell/exec/resize/exit-status matrix can be exercised
/// without a real SSH server.
#[derive(Debug)]
pub struct ScriptedChannel {
    /// Events replayed by `read_event`.
    events: Mutex<VecDeque<ChannelEvent>>,
    /// Last PTY config requested.
    pub last_pty: Mutex<Option<PtyConfig>>,
    /// Last exec command.
    pub last_exec: Mutex<Option<ExecCommand>>,
    /// Last window size.
    pub last_window_size: Mutex<Option<(u16, u16)>>,
    /// Whether a shell was started.
    pub shell_started: Mutex<bool>,
    /// Whether the channel was closed.
    pub closed: Mutex<bool>,
}

impl ScriptedChannel {
    /// Creates a scripted channel with the given event sequence.
    pub fn new(events: Vec<ChannelEvent>) -> Self {
        Self {
            events: Mutex::new(events.into()),
            last_pty: Mutex::new(None),
            last_exec: Mutex::new(None),
            last_window_size: Mutex::new(None),
            shell_started: Mutex::new(false),
            closed: Mutex::new(false),
        }
    }
}

impl SessionChannel for ScriptedChannel {
    async fn request_pty(&self, config: &PtyConfig) -> Result<(), ChannelError> {
        *self.last_pty.lock().expect("pty lock") = Some(config.clone());
        Ok(())
    }

    async fn start_shell(&self) -> Result<(), ChannelError> {
        *self.shell_started.lock().expect("shell lock") = true;
        Ok(())
    }

    async fn exec(&self, command: &ExecCommand) -> Result<(), ChannelError> {
        *self.last_exec.lock().expect("exec lock") = Some(command.clone());
        Ok(())
    }

    async fn set_window_size(&self, columns: u16, rows: u16) -> Result<(), ChannelError> {
        *self.last_window_size.lock().expect("resize lock") = Some((columns, rows));
        Ok(())
    }

    async fn read_event(&self) -> Result<ChannelEvent, ChannelError> {
        self.events
            .lock()
            .expect("events lock")
            .pop_front()
            .ok_or(ChannelError::Closed)
    }

    async fn close(&self) -> Result<(), ChannelError> {
        *self.closed.lock().expect("closed lock") = true;
        Ok(())
    }
}

/// Drives a full interactive-shell session over a channel and returns the
/// collected output and final exit status.
pub async fn run_interactive_shell(
    channel: &impl SessionChannel,
    config: &PtyConfig,
    resize_to: Option<(u16, u16)>,
) -> Result<(Vec<u8>, ExitStatus), ChannelError> {
    channel.request_pty(config).await?;
    channel.start_shell().await?;
    if let Some((columns, rows)) = resize_to {
        channel.set_window_size(columns, rows).await?;
    }
    let mut output = Vec::new();
    let status = loop {
        let event = channel.read_event().await?;
        if let Some(data) = event.data {
            output.extend_from_slice(&data);
        }
        if let Some(exit) = event.exit {
            break exit;
        }
    };
    channel.close().await?;
    Ok((output, status))
}

/// Drives a single-command (exec) session and returns output + exit status.
pub async fn run_exec_command(
    channel: &impl SessionChannel,
    command: &ExecCommand,
) -> Result<(Vec<u8>, ExitStatus), ChannelError> {
    channel.exec(command).await?;
    let mut output = Vec::new();
    let status = loop {
        let event = channel.read_event().await?;
        if let Some(data) = event.data {
            output.extend_from_slice(&data);
        }
        if let Some(exit) = event.exit {
            break exit;
        }
    };
    channel.close().await?;
    Ok((output, status))
}

#[cfg(test)]
mod tests {
    use super::{
        run_exec_command, run_interactive_shell, ChannelEvent, ExecCommand, ExitStatus, PtyConfig,
        ScriptedChannel,
    };
    use core_domain::command::EnvVar;

    fn pty(term: &str) -> PtyConfig {
        PtyConfig::new(term, 80, 24, 640, 480).expect("pty")
    }

    #[tokio::test]
    async fn interactive_shell_with_pty_terminates_and_propagates_exit() {
        let channel = ScriptedChannel::new(vec![
            ChannelEvent::data(b"$ "),
            ChannelEvent::data(b"hello\r\n"),
            ChannelEvent::exit(ExitStatus::success()),
        ]);
        let (output, status) = run_interactive_shell(&channel, &pty("xterm-256color"), None)
            .await
            .expect("session");
        assert_eq!(output, b"$ hello\r\n");
        assert!(status.is_success());
        assert_eq!(
            channel
                .last_pty
                .lock()
                .expect("lock")
                .as_ref()
                .expect("pty")
                .term,
            "xterm-256color"
        );
        assert!(*channel.shell_started.lock().expect("lock"));
        assert!(*channel.closed.lock().expect("lock"));
    }

    #[tokio::test]
    async fn exec_command_propagates_nonzero_exit_status() {
        let channel = ScriptedChannel::new(vec![
            ChannelEvent::data(b"stdout"),
            ChannelEvent::exit(ExitStatus {
                code: Some(42),
                signal: None,
            }),
        ]);
        let command = ExecCommand::new(
            "exit 42",
            vec![EnvVar::new("LANG", "C.UTF-8", false).expect("var")],
        )
        .expect("command");
        let (output, status) = run_exec_command(&channel, &command).await.expect("exec");
        assert_eq!(output, b"stdout");
        assert_eq!(status.code, Some(42));
        assert!(!status.is_success());
        let recorded = channel
            .last_exec
            .lock()
            .expect("lock")
            .clone()
            .expect("exec recorded");
        assert_eq!(recorded.command, "exit 42");
        assert_eq!(recorded.env[0].name, "LANG");
    }

    #[tokio::test]
    async fn resize_is_propagated_between_pty_and_shell() {
        let channel = ScriptedChannel::new(vec![ChannelEvent::exit(ExitStatus::success())]);
        let (_, status) = run_interactive_shell(&channel, &pty("screen"), Some((120, 40)))
            .await
            .expect("session");
        assert!(status.is_success());
        assert_eq!(
            *channel.last_window_size.lock().expect("lock"),
            Some((120, 40))
        );
    }

    #[tokio::test]
    async fn term_matrix_records_each_terminal_type() {
        for term in ["xterm-256color", "vt100", "screen", "xterm"] {
            let channel = ScriptedChannel::new(vec![ChannelEvent::exit(ExitStatus::success())]);
            run_interactive_shell(&channel, &pty(term), None)
                .await
                .expect("session");
            assert_eq!(
                channel
                    .last_pty
                    .lock()
                    .expect("lock")
                    .as_ref()
                    .expect("pty")
                    .term,
                term
            );
        }
    }

    #[tokio::test]
    async fn exec_with_signal_exit_is_not_success() {
        let channel = ScriptedChannel::new(vec![ChannelEvent::exit(ExitStatus {
            code: None,
            signal: Some("SIGKILL".to_owned()),
        })]);
        let command = ExecCommand::new("kill -9 $$", vec![]).expect("command");
        let (_, status) = run_exec_command(&channel, &command).await.expect("exec");
        assert_eq!(status.signal.as_deref(), Some("SIGKILL"));
        assert!(!status.is_success());
    }

    #[tokio::test]
    async fn signal_exit_status_is_reported_for_shell() {
        let channel = ScriptedChannel::new(vec![ChannelEvent::exit(ExitStatus {
            code: None,
            signal: Some("SIGTERM".to_owned()),
        })]);
        let (_, status) = run_interactive_shell(&channel, &pty("xterm"), None)
            .await
            .expect("session");
        assert_eq!(status.signal.as_deref(), Some("SIGTERM"));
        assert!(!status.is_success());
    }

    #[test]
    fn pty_and_exec_validate_input() {
        assert!(PtyConfig::new("", 80, 24, 0, 0).is_err());
        assert!(ExecCommand::new("", vec![]).is_err());
    }

    #[test]
    fn exit_status_success_semantics() {
        assert!(ExitStatus::success().is_success());
        assert!(!ExitStatus {
            code: Some(1),
            signal: None
        }
        .is_success());
        assert!(!ExitStatus {
            code: None,
            signal: Some("SIGINT".to_owned())
        }
        .is_success());
    }
}
