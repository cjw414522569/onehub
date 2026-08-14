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
  'pub struct CancellationToken', 'pub enum CancelReason', 'pub struct Deadline',
  'select_cancellation', 'select_deadline', 'select_guarded',
  'connect_auth_transfer_close_all_cancel_reliably',
  'connect_auth_transfer_close_all_timeout_reliably',
  'select_cancellation_after_many_spawns_has_no_races',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/session-orchestrator/Cargo.toml');
if (!existsSync(join(CRATE, 'src/lib.rs'))) errors.push('Missing crates/session-orchestrator/src/lib.rs');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
const dependencyNames = [];
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  dependencyNames.push(name);
  if (/\{\s*path\s*=/.test(trimmed) && !['core-domain', 'core-errors', 'core-protocol'].includes(name)) {
    errors.push(`session-orchestrator must only path-depend on core crates (found ${name})`);
  }
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`session-orchestrator has forbidden runtime dependency: ${name}`);
}
for (const name of dependencyNames) {
  if (!['core-domain', 'core-errors', 'core-protocol', 'tokio'].includes(name)) {
    errors.push(`session-orchestrator runtime dependency is not approved: ${name}`);
  }
}
if (!dependencyNames.includes('tokio')) errors.push('session-orchestrator must depend on tokio');

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

for (const args of [['check', '-p', 'session-orchestrator', '--locked'], ['test', '-p', 'session-orchestrator', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 180000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p session-orchestrator failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`session-orchestrator concurrency contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('session-orchestrator concurrency contract valid: cancellation token, deadlines, guarded selects, connect/auth/transfer/close cancel + timeout tests, cargo check/test --locked passed.');