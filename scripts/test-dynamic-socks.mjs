#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/proxy');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum AccessPolicy', 'pub struct DynamicSocksConfig', 'pub struct DynamicSocksServer',
  'pub enum DynamicSocksError', 'pub const REP_NOT_ALLOWED', 'pub async fn start',
  'pub async fn stop', 'pub fn allows',
  'connect_ipv4_target_echoes', 'connect_domain_target_echoes',
  'connect_ipv6_literal_target_echoes', 'access_allowlist_permits_and_denies',
  'access_loopback_only_denies_non_loopback', 'required_auth_accepts_correct_credentials',
  'required_auth_rejects_wrong_credentials', 'required_auth_without_offer_is_refused',
  'unreachable_target_gets_connection_refused', 'idle_client_times_out',
  'bind_failure_is_reported', 'access_policy_matrix',
];
const FORBIDDEN_DEPENDENCIES = ['russh', 'libssh', 'ssh2', 'openssh', 'hyper', 'reqwest'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/proxy/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`proxy has forbidden dependency: ${name}`);
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
  if (!sourceText.includes(token)) errors.push(`proxy is missing required token: ${token}`);
}

for (const args of [['check', '-p', 'proxy', '--locked'], ['test', '-p', 'proxy', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p proxy failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`dynamic-socks contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('dynamic-socks contract valid: SOCKS5 server CONNECT for IPv4/domain/IPv6, access policy (allowlist/loopback-only) with REP_NOT_ALLOWED, optional RFC 1929 auth, unreachable targets, idle timeout, bind failure, cargo check/test --locked passed.');