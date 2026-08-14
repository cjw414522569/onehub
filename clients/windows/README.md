# Windows client

- Layer: L5 (native UI shell)
- Approved bridge: `abi-c`
- Status: `windows-first-buildable` skeleton. The Rust host contract builds now; WinUI 3 / Windows App SDK wiring is a later implementation row.
- Forbidden: direct SSH, SFTP, SQLite, secure-store, crypto, or sync-service dependencies.

