#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct ResolvedAddresses', 'pub enum ResolveError', 'pub trait Resolver',
  'pub struct ConnectionGuard', 'pub trait Connector', 'pub enum ConnectError',
  'pub enum ConnectOutcome', 'pub async fn happy_eyeballs_connect', 'pub struct StaticResolver',
  'pub struct CountingConnector', 'HAPPY_EYEBALLS_START_DELAY',
  'v4_fallback_after_v6_failure', 'cancellation_closes_all_open_connections_no_leak',
  'real_loopback_listener_dual_stack',
];
const FORBIDDEN_DEPENDENCIES = [
  'russh', 'russh-keys', 'libssh', 'libssh2', 'ssh2', 'ssh2-sys', 'openssh',
  'winui', 'sqlite', 'gtk', 'flutter', 'tauri',
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
for (const family of ['IpAddr::V4', 'IpAddr::V6']) {
  if (!sourceText.includes(family)) errors.push(`connectivity must handle family: ${family}`);
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`connectivity contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('connectivity contract valid: DNS resolver, Happy Eyeballs v2 (v6-first, v4 fallback, fast failure), IPv4/IPv6, connection timeout/cancellation without socket leak, cargo check/test --locked passed.');