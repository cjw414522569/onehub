# Windows PC GUI client (clients/windows)

- Layer: L5 (native UI shell)
- Approved bridge: `abi-c`
- Status: `windows-first-buildable` — the native host shell builds and runs on Windows.
- Forbidden: direct SSH, SFTP, SQLite, secure-store, crypto, or sync-service dependencies.

## What is implemented

A real, native Windows GUI binary (`ssh-gui`) implemented in Rust with the
`windows-sys` Win32 bindings (GDI text rendering + message loop). All UI
logic lives in the safe, headless-testable model at `src/model.rs`:

- terminal grid (wrap, scroll, resize, color runs)
- input line with cursor editing (insert, backspace, delete, left/right)
- `/connect [user@]host`, `/disconnect`, `/clear`, `/help`, `/quit` commands
- plain input lines are queued as `SendLine` for the abi-c transport
- abi-c `EventBatch` handling (events append output; snapshot recovery)

The Win32 shell (`src/main.rs`) is the documented host-shell boundary: it
only renders the model and feeds it keystrokes. WinUI 3 / Windows App SDK
remains the target toolkit for the full product UI; the Win32 GDI window is
the buildable Windows-first bootstrap.

## Run

```powershell
cargo run -p clients-windows            # open the native GUI window
cargo run -p clients-windows -- --check # headless self-test (CI-safe)
```

In the window: type a command and press Enter. `/connect demo@host` moves the
status bar to `connecting` (transport wiring via abi-c is a later row);
`/quit` closes the window.

## Verify

```powershell
cargo test -p clients-windows --locked
cargo clippy -p clients-windows --all-targets --all-features -- -D warnings
node scripts/test-pc-gui.mjs .
```