# cli

- Layer: L6 (entrypoint)
- Dependencies: `session-orchestrator`, `terminal-parser`, `transfer`, `forwarding`
- Scope: command-line entrypoint over public core contracts.
- T016 status: buildable workspace skeleton; command behavior is deferred to later control rows.



## T101: app startup, database unlock, and failure recovery

`apps/cli/src/startup.rs`:

- `StartupFlow::run` - opens/migrates the database (via storage-sqlite's
  `open_strategy` + `Migrator`) and checks the OS secure store, producing a
  structured `StartupOutcome` with **actionable prompts** for every failure
  mode:
  - `DB_CORRUPTED` - the database file is corrupted; restore a backup.
  - `DB_MIGRATION_FAILED` - a migration step failed; data is preserved at the
    pre-failure version; restore the pre-migration backup.
  - `DB_UNKNOWN_VERSION` - no migration path to the app's target version;
    install the matching app version.
  - `DB_REJECTED` / `DB_READ_ONLY` - version-policy outcomes.
  - `SECURE_STORE_LOCKED` - the OS keychain is locked; unlock and restart.
- Each prompt carries a stable code, severity, title, message, and the
  concrete user action.
- Tests cover the full startup failure matrix (corruption, migration failure,
  unknown version, reject/read-only policies, secure-store lock, healthy
  path).

Dependencies added: `storage-sqlite`, `secure-store` (declared in
`architecture/dependency-rules.json`).

## T143: config, connect, exec, and exit-code contract

`apps/cli/src/cli.rs`:

- `CliConfig` - parses a host-alias config (`[alias]` sections with
  host/user/port/gateway); missing host/user or an invalid port are rejected.
- `CommandRunner` - injectable connect/exec backend (SSH/gateway adapter
  wired in a later row); `MockRunner` keeps the contract deterministic.
- `ExitCode` - stable script exit codes: 0 ok, 1 internal, 2 usage,
  3 config, 4 connect, 5 remote failure.
- Interactive vs non-interactive output separation: non-interactive stdout
  carries only the remote command's stdout (no ANSI, no banners); status
  lines go to stderr (interactive only).
- Real binary E2E: `--version` 0, `--help` 0, bad args 2,
  `config --check` 0/3/3, `exec` 4 (connect backend pending).

## T144: forwarding, SFTP, and proxy-chain capabilities (shared GUI core)

`apps/cli/src/capabilities.rs`:

- `ForwardSpec::to_config` - builds the exact `forwarding::LocalForwardConfig`
  the GUI path uses; `bind_scope` drives the exposure warning.
- `SftpSpec::to_stream_config` - builds the exact `transfer::StreamConfig`
  (defaults equal the core defaults); `run_sftp_copy` runs the shared
  `run_streaming_copy` engine.
- `ProxyChainSpec::first_hop_wire` - builds the SOCKS5 greeting + CONNECT
  wire bytes with the shared `proxy` encoders (byte-identical to the GUI).
- CLI/GUI core-agreement tests: config equality, byte-identical wire, and
  matching transfer statistics ? no behavior divergence.
- Real binary `cap` commands: `forward`, `sftp`, `proxy --chain`.
