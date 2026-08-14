#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/forwarding');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct RemoteForwardConfig', 'pub enum RemoteForwardReply', 'pub enum RemoteForwardEvent',
  'pub enum RemoteForwardError', 'pub trait RemoteForwardPeer', 'pub struct RemoteForwarder',
  'pub struct WirePeer', 'pub async fn establish', 'pub async fn pipe_incoming',
  'pub fn mark_server_closed', 'pub fn allocated_port', 'pub fn is_listening',
  'pub fn encode_tcpip_forward_request', 'pub fn decode_global_request',
  'pub fn encode_request_success', 'pub fn encode_request_failure', 'pub fn decode_request_reply',
  'dynamic_port_allocation_via_wire', 'rejection_is_visible_via_wire',
  'server_close_is_visible', 'incoming_connection_is_piped_to_local_target',
  'wire_peer_maps_failure_reply_to_rejected', 'tcpip_forward_request_round_trip',
  'request_reply_round_trip', 'malformed_requests_are_rejected',
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
  console.error(`remote-forwarding contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('remote-forwarding contract valid: RFC 4254 7.1 tcpip-forward codec over the real wire, dynamic port allocation, rejection visibility, server-close visibility, incoming piped to local target, cargo check/test --locked passed.');