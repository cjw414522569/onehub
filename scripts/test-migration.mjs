#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SQLITE = join(ROOT, 'crates/storage-sqlite');
const errors = [];

// T083: SQLite schema, migration rules, backup, downgrade policy.
const SQLITE_TOKENS = [
  'pub struct Migrator', 'pub struct Migration', 'pub struct MigrationContext',
  'pub struct SchemaVersion', 'pub enum MigrationError', 'pub enum BackupMode',
  'pub struct BackupPolicy', 'pub enum OpenPolicy', 'pub enum OpenDecision',
  'pub fn open_strategy', 'pub fn migrate', 'pub fn rollback', 'pub fn current_version',
  'pub apply', 'pub revert',
  'migration_applies_in_order_and_is_idempotent', 'failing_step_rolls_back_transactionally',
  'rollback_reverts_in_reverse', 'rollback_missing_revert_fails',
  'backup_policy_sets_path_before_migration', 'open_strategy_handles_old_and_new_versions',
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
  console.error(`migration contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('migration contract valid: Migrator applies steps transactionally and idempotently (already-applied versions skipped), failed steps roll back their own record; rollback runs revert steps in reverse with NoRevert detection; BackupPolicy sets pre-migration backup path; OpenPolicy makes the old-version open strategy explicit (Upgrade/ReadOnly/Reject); full version migration/rollback tests pass; cargo check/test --locked passed.');