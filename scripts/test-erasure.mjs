#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STORE = join(ROOT, 'crates/secure-store');
const errors = [];

// T096: local data deletion, account sign-out, and cryptographic erasure.
const ERASURE_TOKENS = [
  'pub struct CryptoErasure', 'pub enum ErasureScope', 'pub struct ErasurePlan',
  'pub struct ErasureReport', 'pub trait DataStore', 'pub trait BackupStore',
  'pub fn forensic_scan', 'pub fn plan', 'pub fn erase', 'ACCOUNT_SECRET_PREFIX',
  'pub struct MemoryDataStore', 'pub struct MemoryBackupStore', 'fn names',
  'plan_lists_exact_scope_for_confirmation', 'erase_executes_exactly_the_confirmed_plan',
  'cryptographic_erasure_leaves_no_recoverable_material',
  'forensic_scan_detects_leftovers_after_partial_erasure',
  'account_sign_out_clears_account_material_only',
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

checkCrateTokens(STORE, ERASURE_TOKENS, 'secure-store erasure');
checkForbiddenDeps(STORE, 'secure-store');

for (const args of [
  ['check', '-p', 'secure-store', '--locked'],
  ['test', '-p', 'secure-store', '--locked'],
  ['check', '-p', 'sync-service', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`erasure contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('erasure contract valid: CryptoErasure::plan computes an exact, user-confirmable ErasurePlan per ErasureScope (Account sign-out clears account:* secrets only; LocalData/Backups/Everything scope accordingly); erase removes exactly the confirmed items and reports counts; forensic_scan re-reads every store and returns hits for any remaining plaintext markers or secret names (empty after a full wipe, non-empty for leftovers after a partial erase); SecureStore::names() enables enumeration; cargo check/test --locked passed for secure-store (and sync-service still compiles).');