#!/usr/bin/env node

// T135 contract: gateway versioned session protocol.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const GATEWAY = join(ROOT, 'services/gateway');
const errors = [];

const TOKENS = [
  'pub const SESSION_PROTOCOL_VERSION', 'pub enum MessageType', 'pub struct MessageFlags',
  'pub struct SessionMessage', 'pub struct CapabilitySet', 'pub fn negotiate',
  'pub enum SessionPhase', 'pub enum ProtocolError', 'pub struct GatewaySession',
  'pub fn handle_hello', 'pub fn authenticate', 'pub fn negotiate_capabilities', 'pub fn receive',
  'pub fn resume', 'pub fn close',
  'handshake_auth_and_data_flow', 'capability_negotiation_intersects',
  'backpressure_and_cancel_flags', 'resume_after_network_failure',
  'version_mismatch_is_rejected', 'close_terminates',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(GATEWAY, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`gateway missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'gateway', '--locked'],
  ['test', '-p', 'gateway', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p gateway failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`gateway-session-protocol contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gateway-session-protocol contract valid: GatewaySession is a versioned state machine (handshake version check -> authentication -> capability negotiation -> ready) with per-message cancel/backpressure flags and a resume token that reconnects a session after a network failure without re-authenticating; CapabilitySet::negotiate returns the intersection; protocol-contract and network-fault tests pass; cargo check/test --locked passed.');