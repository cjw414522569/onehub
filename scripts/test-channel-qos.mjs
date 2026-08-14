#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum TrafficClass', 'pub struct FlowWindow', 'pub enum QosError',
  'pub struct SchedulerConfig', 'pub struct Scheduler', 'pub struct ScheduledSend',
  'pub struct ChannelSnapshot', 'pub struct SchedulerSnapshot',
  'fn register', 'fn unregister', 'fn set_class', 'fn enqueue', 'fn window_adjust',
  'pub fn drain', 'fn try_send', 'fn try_send_bulk', 'pub fn snapshot',
  'interactive_is_scheduled_before_bulk', 'bulk_channels_share_fairly_and_none_starves',
  'flow_window_blocks_and_resumes_on_adjust', 'window_adjust_is_capped_at_max',
  'round_budget_limits_each_drain', 'class_change_moves_channel_to_priority',
  'qos_benchmark_concurrent_terminal_sftp_forwarding',
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
if (!existsSync(libRs) || !readFileSync(libRs, 'utf8').includes('pub mod channel_qos;')) {
  errors.push('lib.rs does not register pub mod channel_qos;');
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`channel-qos contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('channel-qos contract valid: strict-priority + DRR fair scheduler, per-channel flow window (RFC 4254 5.2) with cap, budget-bounded drain, concurrent terminal/SFTP/forwarding QoS benchmark, cargo check/test --locked passed.');