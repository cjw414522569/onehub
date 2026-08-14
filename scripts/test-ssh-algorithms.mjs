#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct SshVersion', 'pub enum SshVersionError', 'pub enum AlgorithmKind',
  'pub enum Algorithm', 'pub enum AlgorithmSecurity', 'pub struct AlgorithmPolicy',
  'pub fn negotiate_algorithm', 'pub enum NegotiatedAlgorithm', 'pub struct HostAlgorithmPolicy',
  'downgrade_attack_is_rejected',
  'per_host_override_enables_legacy_only_for_that_host',
  'defaults_are_secure_and_exclude_sha1_and_cbc',
  'matrix_covers_all_kinds',
];
const FORBIDDEN_DEPENDENCIES = [
  'russh', 'russh-keys', 'libssh', 'libssh2', 'ssh2', 'ssh2-sys', 'openssh',
];

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
// Insecure algorithms must be present but disabled by default.
for (const weak of ['Group1Sha1', 'Group14Sha1', 'SshRsa', 'Aes128Cbc', 'TripleDesCbc', 'HmacSha1']) {
  if (!sourceText.includes(weak)) errors.push(`algorithms module must cover insecure algorithm: ${weak}`);
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`ssh algorithms contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('ssh algorithms contract valid: version exchange, secure defaults, algorithm matrix, downgrade rejection, per-host explicit compatibility, cargo check/test --locked passed.');