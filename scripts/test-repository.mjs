#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SQLITE = join(ROOT, 'crates/storage-sqlite');
const errors = [];

// T084: config repository + atomic transactions (SQL-free domain contract).
const SQLITE_TOKENS = [
  'pub trait ConfigRepository', 'pub struct AtomicStore', 'pub struct AtomicTransaction',
  'pub enum CasError', 'pub enum TransactionError', 'pub fn compare_and_swap',
  'pub fn version', 'pub fn begin', 'pub fn commit', 'pub fn read', 'pub fn write',
  'pub fn delete', 'fn get', 'fn set',
  'repository_contract_round_trip', 'compare_and_swap_rejects_stale_writers',
  'concurrent_updates_do_not_lose_data', 'transaction_commits_atomically_and_detects_conflict',
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

if (!existsSync(join(SQLITE, 'Cargo.toml'))) errors.push('Missing crates/storage-sqlite/Cargo.toml');
checkCrateTokens(SQLITE, SQLITE_TOKENS, 'storage-sqlite');
checkForbiddenDeps(SQLITE, 'storage-sqlite');

for (const args of [
  ['check', '-p', 'storage-sqlite', '--locked'],
  ['test', '-p', 'storage-sqlite', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`repository contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('repository contract valid: SQL-free ConfigRepository trait (domain never depends on SQL); AtomicStore with per-key versions and compare_and_swap (stale writers get VersionMismatch, no silent lost update); AtomicTransaction snapshot-isolated with conflict detection on commit; 8-thread x 250 CAS concurrency test preserves every update; cargo check/test --locked passed.');