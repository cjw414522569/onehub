# sftp-backend

- Layer: L2 (protocol adapters)
- Dependencies: `core-domain`, `core-protocol`, `transfer`
- Scope: replaceable SFTP adapter boundary.
- T016 status: buildable workspace skeleton; concrete engine work is deferred to its control row.

## T056: SFTP v3 subsystem basics and capability probing

| Module | Purpose |
|---|---|
| `attrs` | `FileAttrs` (size/uid/gid/permissions/atime/mtime) codec + POSIX type and mode-string helpers. |
| `msg` | SFTP v3 packet framing, message types, request encoders and response decoders (init/version/open/close/read/write/lstat/fstat/setstat/opendir/readdir/remove/mkdir/rmdir/realpath/stat/rename/readlink/symlink/extended/status/handle/data/name/attrs). |
| `client` | `SftpClient`: INIT/VERSION handshake with `SftpCapabilities` (extension probing), typed operations, `SftpStatus` mapping. |
| `server` | In-memory `VirtualFs` + `SftpServer` implementing the full op set with `SSH_FX_*` status codes (real OpenSSH SFTP is `blocked_environment` on this host). |

Integration tests over duplex: capability probing (posix-rename/statvfs/lsetstat),
mkdir/list/stat, write/read/fstat/truncate, rename/delete with status codes
(NoSuchFile/Failure), posix-rename extension, permissions (drwxr-xr-x) and
symlink lstat/stat/readlink semantics, realpath canonicalization, and
unsupported-extension reporting.
