#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SECURE = join(ROOT, 'crates/secure-store');
const errors = [];

// T086: Apple Keychain / Secure Enclave adapter.
const SECURE_TOKENS = [
  'pub enum AccessControl', 'pub enum Biometrics', 'pub struct AccessPolicy',
  'pub struct AppleKeychainStore', 'pub struct MemoryKeychainStore', 'pub enum MigrationOutcome',
  'pub fn migrate_keychain', 'pub fn write_with_policy', 'pub fn policies_applied',
  'pub fn policy', 'WhenUnlocked', 'AfterFirstUnlock', 'TouchId', 'FaceId', 'device_only',
  'access_policy_defaults_and_custom', 'apple_adapter_delegates_and_keeps_policy',
  'keychain_double_records_applied_policy', 'migration_moves_legacy_items_under_new_policy',
  'migration_with_no_legacy_items_is_a_noop',
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

if (!existsSync(join(SECURE, 'Cargo.toml'))) errors.push('Missing crates/secure-store/Cargo.toml');
checkCrateTokens(SECURE, SECURE_TOKENS, 'secure-store');
checkForbiddenDeps(SECURE, 'secure-store');

for (const args of [
  ['check', '-p', 'secure-store', '--locked'],
  ['test', '-p', 'secure-store', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`apple-keychain contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('apple-keychain contract valid: AppleKeychainStore targets the Keychain with an explicit AccessPolicy (access control + biometrics + device-only); MemoryKeychainStore records per-item applied policy; migrate_keychain moves legacy items to a new service prefix under the new policy (legacy copy removed, unrelated items untouched); cargo check/test --locked passed.');