#!/usr/bin/env node

// T113 contract: port forwarding management + occupancy diagnosis.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum ForwardKind', 'pub enum ForwardState', 'pub enum ForwardRisk',
  'pub struct ForwardRiskWarning', 'pub struct PortForward', 'pub fn address_label',
  'pub fn risk_warning', 'pub enum ForwardError', 'pub enum OccupancyStatus',
  'pub struct ForwardManager', 'pub fn set_occupied_ports', 'pub fn diagnose', 'pub fn create',
  'pub fn pause', 'pub fn resume', 'pub fn reconnect', 'pub fn confirm_reconnected',
  'pub fn remove', 'pub fn list',
  'create_local_remote_dynamic_forwards', 'occupied_port_create_fails_with_message',
  'pause_resume_reconnect_cycle', 'risk_warnings_wildcard_and_privileged',
  'address_label_is_copy_ready', 'remove_frees_forward',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(LIB, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`host-library missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'host-library', '--locked'],
  ['test', '-p', 'host-library', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p host-library failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`port-forwarding contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('port-forwarding contract valid: ForwardManager creates local/remote/dynamic forwards (occupied or invalid listen ports fail with an actionable message via the occupancy diagnostic), and drives pause / resume / reconnect / confirm / remove; PortForward has a copy-ready address_label and risk warnings (all-interfaces listen = high, privileged port = medium); diagnose reports free/occupied status; create/pause/resume/reconnect/copy/risk tests pass; cargo check/test --locked passed.');