# Windows PC GUI client (clients/windows)

- Layer: L5 (native UI shell)
- Approved bridge: `abi-c`
- Status: `windows-first-buildable` — the native host shell builds and runs on Windows.
- Forbidden: direct SSH, SFTP, SQLite, secure-store, crypto, or sync-service dependencies.

## What is implemented

A real, native Windows GUI binary (`onehub`) implemented in Rust with the
`windows-sys` Win32 bindings (GDI text rendering + message loop). The chrome
references the mXterm light-neutral design (`mxterm/prototype`): a top
connection-tab bar with an add (`+`) tab, a left session repository, a dark
terminal area, a light input line and status bar, and a modal "new SSH"
dialog. All UI logic lives in the safe, headless-testable model at
`src/model.rs`:

- terminal grid (wrap, scroll, resize, color runs)
- input line with cursor editing (insert, backspace, delete, left/right)
- `/connect [user@]host`, `/disconnect`, `/sessions`, `/open <index>`,
  `/clear`, `/help`, `/quit` commands
- session repository: `ConnectionProfile` (name/host/port/user), add/select/
  remove/connect; credentials are deliberately never persisted
- plain input lines are queued as `SendLine` for the abi-c transport
- abi-c `EventBatch` handling (events append output; snapshot recovery)

The Win32 shell (`src/main.rs`) is the documented host-shell boundary: it
only renders the model and feeds it keystrokes/mouse input. WinUI 3 /
Windows App SDK remains the target toolkit for the full product UI; the
Win32 GDI window is the buildable Windows-first bootstrap.

## Run

```powershell
cargo run -p clients-windows            # open the native GUI window
cargo run -p clients-windows -- --check # headless self-test (CI-safe)
```

In the window: click `+` to open the new-SSH dialog (name/host/port/user,
credentials are not persisted), click a tab or double-click a session row to
connect, type a command in the input line and press Enter. `/quit` closes
the window.

## Verify

```powershell
cargo test -p clients-windows --locked
cargo clippy -p clients-windows --all-targets --all-features -- -D warnings
node scripts/test-pc-gui.mjs .
```