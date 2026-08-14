#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct AgentForwardController', 'pub struct AgentForwardRisk',
  'pub enum AgentForwardState', 'pub enum AgentForwardOutcome', 'pub enum AgentForwardError',
  'pub struct AgentForwardTransport', 'pub trait AgentForwardPeer',
  'pub fn encode_agent_channel_open', 'pub fn decode_agent_channel_open',
  'pub fn encode_channel_open_confirmation', 'pub fn encode_channel_open_failure',
  'pub fn encode_channel_close', 'AGENT_FORWARD_CHANNEL_TYPE', 'AGENT_FORWARD_RISK_CODE',
  'agent_forwarding_is_off_by_default', 'per_session_authorization_is_explicit',
  'authorization_returns_risk_notice_for_ui', 'channel_open_codec_round_trip',
  'wire_open_confirmed', 'wire_open_rejected', 'wire_disconnect_on_channel_close',
  'agent_frames_round_trip_through_channel',
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
if (!existsSync(libRs) || !readFileSync(libRs, 'utf8').includes('pub mod agent_forwarding;')) {
  errors.push('lib.rs does not register pub mod agent_forwarding;');
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`agent-forwarding contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('agent-forwarding contract valid: off by default, explicit per-session authorization, UI risk notice, RFC 4254 auth-agent channel open/confirm/failure/close over the wire, agent-frame round trip, cargo check/test --locked passed.');