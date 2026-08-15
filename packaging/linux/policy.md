# Linux packaging policy (T126)

Reproducible packages for deb / rpm / AppImage / Flatpak with clear
dependencies, sandbox permissions, and auto-update boundaries.

## Formats

- **deb** (Debian / Ubuntu) - depends on `libc6`; no sandbox; updates only
  via the distro repository (no self-update).
- **rpm** (Fedora / RHEL) - depends on `glibc`; no sandbox; updates only via
  the distro repository.
- **AppImage** (any Linux) - depends on FUSE 2/3; no sandbox; no auto-update
  (manual download).
- **Flatpak** (Flathub) - depends on `org.freedesktop.Platform`; sandboxed
  with a minimal permission set (network, IPC share, devices only if
  needed); updates ride the Flatpak runtime with versioned app data.

## Boundaries

- Auto-update never bypasses the packaging channel: repository packages
  update via the repository, Flatpak via the runtime, AppImage not at all.
- Reproducibility: packages are built in clean CI with source timestamps and
  deterministic archives (regenerate -> no diff).

`packaging/linux/policy.json` is the machine-readable manifest; it is
linted (`scripts/lint-linux-packaging.mjs`) and snapshot-tested
(`scripts/test-linux-packaging.mjs`). Install / upgrade / uninstall on clean
distros runs on Linux hosts.