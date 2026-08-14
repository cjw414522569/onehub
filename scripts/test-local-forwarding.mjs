#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/forwarding');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum BindScope', 'pub struct LocalForwardConfig', 'pub enum ForwardError',
  'pub trait TargetConnector', 'pub struct TcpConnector', 'pub struct LocalForwarder',
  'pub async fn start', 'pub async fn close_listener', 'pub async fn drain',
  'pub async fn stop', 'pub fn requires_bind_warning', 'pub fn active_connections',
  'tcp_echo_round_trip_through_local_forward', 'concurrent_connections_all_echo_with_cap',
  'connection_cap_refuses_excess_clients', 'graceful_shutdown_closes_listener_and_drains',
  'bind_failure_is_reported', 'non_local_bind_address_is_reported',
  'bind_scope_warning_policy', 'same_listen_endpoint_conflicts_via_forwarding_table',
  'stop_without_start_is_reported',
];
const FORBIDDEN_DEPENDENCIES = ['russh', 'libssh', 'ssh2', 'openssh'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/forwarding/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`forwarding has forbidden dependency: ${name}`);
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
  if (!sourceText.includes(token)) errors.push(`forwarding is missing required token: ${token}`);
}

for (const args of [['check', '-p', 'forwarding', '--locked'], ['test', '-p', 'forwarding', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p forwarding failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`local-forwarding contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('local-forwarding contract valid: bind-scope warning, concurrent-connection cap with EOF refusal, graceful shutdown (close_listener + drain), TCP echo, bind failure (occupied port / non-local address), T031 conflict detection, cargo check/test --locked passed.');