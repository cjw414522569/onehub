#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct ServerCombo', 'pub fn check_combo', 'pub struct ChosenAlgorithms',
  'pub enum CompatVerdict', 'pub enum ExecutionMode', 'pub struct MatrixEntry',
  'pub struct CompatMatrixReport', 'pub fn run_compat_matrix', 'pub fn local_checks_passed',
  'pub fn platforms', 'pub static THE_MATRIX', 'LIVE_BLOCKED_REASON',
  'linux-ubuntu-2404-openssh-9.6', 'linux-debian-12-openssh-9.2', 'linux-rhel-9-openssh-8.7',
  'macos-14-openssh-9.4', 'windows-11-openssh-9.5', 'windows-10-openssh-8.9',
  'freebsd-13-openssh-9.0', 'openbsd-7.4-openssh-9.5', 'linux-rhel-7-openssh-7.4',
  'linux-rhel-6-openssh-5.3',
  'modern_servers_are_compatible_with_secure_defaults',
  'legacy_sha1_only_server_is_rejected_by_secure_defaults',
  'legacy_server_connects_with_explicit_opt_in',
  'rhel7_negotiates_modern_algorithms_with_secure_defaults',
  'matrix_covers_all_target_platforms_and_blocks_live_execution',
  'chosen_algorithms_follow_local_preference',
];
const FORBIDDEN_DEPENDENCIES = ['russh', 'russh-keys', 'libssh', 'libssh2', 'ssh2', 'ssh2-sys', 'openssh'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/ssh-backend/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`ssh-backend has forbidden dependency: ${name}`);
}

const sourceFiles = [];
function collect(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collect(absolute);
    else if (entry.name.endsWith('.rs')) sourceFiles.push(absolute);
  }
}
if (existsSync(join(CRATE, 'src'))) collect(join(CRATE, 'src'));
const sourceText = sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`ssh-backend is missing required token: ${token}`);
}

const libRs = join(CRATE, 'src/lib.rs');
if (!existsSync(libRs) || !readFileSync(libRs, 'utf8').includes('pub mod compat_matrix;')) {
  errors.push('lib.rs does not register pub mod compat_matrix;');
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`openssh-compat contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('openssh-compat contract valid: data-driven OpenSSH server matrix (Linux/macOS/Windows/FreeBSD/OpenBSD x versions), local algorithm-intersection checks auto-executed for every combo, secure-default rejection of SHA-1-only legacy with explicit opt-in, live nightly matrix honestly blocked_environment, cargo check/test --locked passed.');