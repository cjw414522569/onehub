#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const FORBIDDEN_BACKEND_DEPENDENCIES = [
  'russh', 'russh-keys', 'libssh', 'libssh2', 'ssh2', 'ssh2-sys',
  'openssh', 'thrussh', 'async-ssh2',
];
const REQUIRED_TOKENS = [
  'pub struct SessionTarget', 'pub enum TransportError', 'pub struct SessionHandle',
  'pub trait SshTransport', 'pub struct FakeTransport', 'pub enum FakeTransportMode',
  'async fn connect', 'fn name',
  'fake_connect_returns_opaque_handle',
  'fake_slow_connect_honours_cancellation',
  'transport_error_codes_are_unique_and_stable',
];
const FORBIDDEN_IMPORT_TOKENS = ['russh', 'libssh', 'ssh2', 'winui', 'sqlite', 'gtk'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/ssh-backend/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_BACKEND_DEPENDENCIES.includes(name)) {
    errors.push(`ssh-backend must not depend on a concrete SSH library yet: ${name}`);
  }
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
for (const token of FORBIDDEN_IMPORT_TOKENS) {
  // Reject actual imports or path-qualified usages, not prose mentions.
  const importPattern = new RegExp(`use\\s+${token}(::|\\b)`);
  const qualifierPattern = new RegExp(`\\b${token}::`);
  if (importPattern.test(sourceText) || qualifierPattern.test(sourceText)) {
    errors.push(`ssh-backend source must not import or use backend/library token: ${token}`);
  }
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`ssh adapter contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('ssh adapter contract valid: injectable SshTransport, opaque SessionHandle, fake transport tests, no concrete SSH library in the domain boundary, cargo check/test --locked passed.');