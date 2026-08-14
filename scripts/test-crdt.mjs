#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SYNC = join(ROOT, 'crates/sync-core');
const errors = [];

// T093: local sync CRDT / conflict-merge core.
const SYNC_TOKENS = [
  'pub struct LamportClock', 'pub struct CrdtEntry', 'pub struct CrdtState', 'pub fn set',
  'pub fn delete', 'pub fn get', 'pub fn is_tombstone', 'pub fn merge', 'pub fn converge',
  'pub type ReplicaId', 'tombstone',
  'set_get_delete_recover', 'offline_concurrent_edits_converge_deterministically',
  'random_multi_replica_property_converges', 'delete_is_recoverable_under_concurrency',
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

if (!existsSync(join(SYNC, 'Cargo.toml'))) errors.push('Missing crates/sync-core/Cargo.toml');
checkCrateTokens(SYNC, SYNC_TOKENS, 'sync-core');
checkForbiddenDeps(SYNC, 'sync-core');

for (const args of [
  ['check', '-p', 'sync-core', '--locked'],
  ['test', '-p', 'sync-core', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`crdt contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('crdt contract valid: per-key LWW register CRDT with Lamport clocks and tombstones; offline concurrent edits converge deterministically (merge is commutative/idempotent), deletes are recoverable (tombstones superseded by newer sets); random multi-replica property tests converge in any merge order; cargo check/test --locked passed.');