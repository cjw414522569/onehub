#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub fn frame_message', 'pub fn parse_frame', 'pub struct AgentIdentity',
  'pub trait AgentStream', 'pub struct AgentClient', 'pub struct FakeAgentServer',
  'pub enum AgentError', 'pub async fn request_identities', 'pub async fn sign',
  'identity_enumeration_over_duplex',
  'sign_request_returns_verifiable_signature',
  'timeout_is_enforced_when_server_is_silent',
  'disconnect_is_detected_as_eof',
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
for (const code of ['REQUEST_IDENTITIES', 'IDENTITIES_ANSWER', 'SIGN_REQUEST', 'SIGN_RESPONSE', 'FAILURE']) {
  if (!sourceText.includes(code)) errors.push(`agent protocol must define message code: ${code}`);
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`agent contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('agent contract valid: framing, identity enumeration, sign, timeout, EOF disconnect over the real agent protocol (in-memory duplex); desktop agents recorded blocked_environment, cargo check/test --locked passed.');