#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/session-orchestrator');
const errors = [];

const FORBIDDEN_DEPENDENCIES = [
  'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui', 'windows-app-sdk',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'wgpu', 'harfbuzz',
];
const FORBIDDEN_IMPORT_TOKENS = [
  'winui', 'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'webview', 'russh', 'libssh', 'sqlx', 'sqlite', 'storage-sqlite', 'secure-store',
];
const REQUIRED_TOKENS = [
  'pub struct BoundedChannel', 'pub enum SlowConsumerPolicy', 'pub struct ChannelStats',
  'one_gib_synthetic_output_stays_bounded_and_non_blocking',
  'block_policy_applies_backpressure',
  'drop_oldest_evicts_oldest_and_counts_evictions',
  'drop_newest_never_blocks_and_counts_drops',
  'close_wakes_blocked_receivers_and_send_fails',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/session-orchestrator/Cargo.toml');

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
for (const token of FORBIDDEN_IMPORT_TOKENS) {
  if (new RegExp(`\\b${token}\\b`, 'i').test(sourceText)) {
    errors.push(`session-orchestrator source references forbidden token: ${token}`);
  }
}
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`session-orchestrator is missing required token: ${token}`);
}
for (const policy of ['Block', 'DropNewest', 'DropOldest']) {
  if (!sourceText.includes(policy)) errors.push(`SlowConsumerPolicy is missing variant: ${policy}`);
}

for (const args of [['check', '-p', 'session-orchestrator', '--locked'], ['test', '-p', 'session-orchestrator', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p session-orchestrator failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`bounded channel contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('bounded channel contract valid: Block/DropNewest/DropOldest policies, close semantics, stats, 1 GiB synthetic-output stress test, cargo check/test --locked passed.');