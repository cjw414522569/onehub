#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const errors = [];

// T073: incremental terminal-state merge and dirty-line tracking. Per-frame
// batching via DirtyTracker + DeltaBuilder -> TerminalDelta; dropped frames
// recoverable by full snapshot; incremental/full equivalence property tests.
const STATE_TOKENS = [
  'pub struct DirtyTracker', 'pub fn mark_row', 'pub fn mark_cursor', 'pub fn mark_title',
  'pub fn mark_working_directory', 'pub fn dirty_rows', 'pub fn clear',
  'pub struct DeltaBuilder', 'pub fn build', 'pub fn apply_delta', 'pub fn diff_rows',
  'pub fn blank_snapshot', 'pub enum DeltaError', 'SequenceGap', 'MissingSnapshot',
  'TerminalDelta', 'DeltaOp::Fill',
  'dirty_tracker_accumulates_and_clears', 'delta_build_and_apply_equivalence',
  'incremental_diff_and_merge_equivalence', 'dropped_frame_recovers_from_full_snapshot',
  'sequence_gap_is_detected', 'frame_batches_all_dirty_rows',
];
const FORBIDDEN_DEPENDENCIES = ['vt-parser', 'wezterm-term', 'alacritty_terminal'];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

function checkCrateTokens(crateDir, tokens, label) {
  const files = [];
  if (existsSync(join(crateDir, 'src'))) collectRs(join(crateDir, 'src'), files);
  if (existsSync(join(crateDir, 'tests'))) collectRs(join(crateDir, 'tests'), files);
  const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
  for (const token of tokens) {
    if (!sourceText.includes(token)) errors.push(`${label} is missing required token: ${token}`);
  }
}

function checkForbiddenDeps(crateDir, label) {
  const manifest = readFileSync(join(crateDir, 'Cargo.toml'), 'utf8');
  const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
  const depsSection = depsMatch?.[1] ?? '';
  for (const line of depsSection.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
    if (!name) continue;
    if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`${label} has forbidden dependency: ${name}`);
  }
}

if (!existsSync(join(STATE, 'Cargo.toml'))) errors.push('Missing crates/terminal-state/Cargo.toml');
checkCrateTokens(STATE, STATE_TOKENS, 'terminal-state');
checkForbiddenDeps(STATE, 'terminal-state');

for (const args of [
  ['check', '-p', 'terminal-state', '--locked'],
  ['test', '-p', 'terminal-state', '--locked'],
  ['check', '-p', 'terminal-parser', '--locked'],
  ['test', '-p', 'terminal-parser', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`delta contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('delta contract valid: DirtyTracker accumulates per-frame dirty rows/cursor/title/wd; DeltaBuilder batches them into one core-protocol TerminalDelta per frame; apply_delta merges (Fill/Copy/Clear/Cursor/Title/Image, grapheme-safe with wide continuations); diff_rows tracks receiver changes; SequenceGap/MissingSnapshot detected so dropped frames recover from a full snapshot; incremental/full equivalence + dropped-frame recovery property tests pass; cargo check/test --locked passed.');