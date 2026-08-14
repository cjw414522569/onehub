# SSH engine fixtures

The spike intentionally does not create or persist private keys, passwords, hostnames, or terminal content.

For a daemon-backed interoperability run, provide an isolated OpenSSH `sshd` fixture outside this repository and rerun the same matrix with its endpoint. The current Windows host has the Git for Windows OpenSSH client (`OpenSSH_10.3p1`) but no `sshd.exe`, so the report records daemon-backed interoperability as an environment limitation rather than fabricating a pass.

The Rust candidates use their package-provided unit/API evidence and build commands. `russh` is checked with both its default `aws-lc-rs` backend and the portable `ring` backend; `ssh2` uses the vendored `libssh2-sys` path; `libssh-rs` records the missing native OpenSSL/Perl prerequisites.
