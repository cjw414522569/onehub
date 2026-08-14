#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/session-orchestrator');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum SessionState', 'pub enum SessionEvent', 'pub enum SessionEffect',
  'pub struct SessionTransition', 'pub enum SessionTransitionResult',
  'pub fn apply', 'pub fn replay', 'pub fn replay_from', 'pub struct SessionSnapshot',
  'every_state_event_pair_has_a_deterministic_outcome',
  'acceptance_flow_is_unambiguous',
  'closed_is_terminal_and_rejects_every_event',
];

const FORBIDDEN_DEPENDENCIES = [
  'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui', 'windows-app-sdk',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'wgpu', 'harfbuzz',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/session-orchestrator/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`session-orchestrator has forbidden dependency: ${name}`);
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
  if (!sourceText.includes(token)) errors.push(`session-orchestrator is missing required token: ${token}`);
}
for (const state of ['Disconnected', 'Connecting', 'Authenticating', 'Online', 'Reconnecting', 'Suspended', 'Closing', 'Closed']) {
  if (!sourceText.includes(state)) errors.push(`SessionState is missing variant: ${state}`);
}

for (const args of [['check', '-p', 'session-orchestrator', '--locked'], ['test', '-p', 'session-orchestrator', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p session-orchestrator failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`session state machine contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('session state machine contract valid: 8 states, event-sourced apply/replay, exhaustive pair test, cargo check/test --locked passed.');