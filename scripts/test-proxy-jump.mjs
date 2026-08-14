#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum HopErrorKind', 'pub struct HopError', 'pub struct HopEndpoint',
  'pub trait HopSession', 'pub struct EstablishedHop', 'pub trait MultiHopBackend',
  'pub struct HopRecord', 'pub struct MultiHopReport', 'pub async fn connect_chain',
  'pub type HopResolver', 'E_HOP_INVALID_CHAIN', 'E_HOP_RESOLVE', 'E_HOP_CONNECT',
  'E_HOP_HOST_KEY_REJECTED', 'E_HOP_AUTH_FAILED', 'E_HOP_TUNNEL_OPEN', 'E_HOP_CANCELLED',
  'E_HOP_TIMEOUT', 'fn connect_first', 'fn connect_next', 'fn open_tunnel',
  'single_jump_topology_connects', 'two_hop_topology_connects', 'three_hop_topology_connects',
  'host_key_rejection_is_localized_to_that_hop', 'per_hop_credential_failure_is_localized',
  'tunnel_unreachable_hop_is_localized', 'connect_failure_is_localized_to_hop_index',
  'unresolved_hop_is_reported_as_resolve_error', 'cyclic_chain_is_rejected',
  'pre_cancelled_token_stops_at_first_hop', 'per_hop_timeout_policy_is_enforced',
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

const libRs = join(CRATE, 'src/lib.rs');
if (!existsSync(libRs) || !readFileSync(libRs, 'utf8').includes('pub mod proxy_jump;')) {
  errors.push('lib.rs does not register pub mod proxy_jump;');
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`proxy-jump contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('proxy-jump contract valid: 1/2/3-hop in-process topology, per-hop independent host-key verification and credentials, per-hop timeout, hop-localized errors (E_HOP_* stable codes), cancellation and cycle rejection, cargo check/test --locked passed.');