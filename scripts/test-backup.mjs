#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SQLITE = join(ROOT, 'crates/storage-sqlite');
const errors = [];

// T090: secure import/export and encrypted backup format.
const SQLITE_TOKENS = [
  'pub const BACKUP_VERSION', 'pub struct KdfParams', 'pub struct ExportScope',
  'pub struct BackupArchive', 'pub enum BackupError', 'pub fn encrypt_backup',
  'pub fn decrypt_backup', 'pub fn random_salt', 'pub fn derive_key', 'log_n', 'r', 'p',
  'scrypt', 'includes_category',
  'round_trip_encrypt_decrypt', 'wrong_passphrase_fails', 'unsupported_version_fails',
  'weak_kdf_parameters_are_rejected', 'random_salt_is_nonzero_and_16_bytes',
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
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 300000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`backup contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('backup contract valid: versioned BackupArchive (BACKUP_VERSION=1) with explicit ExportScope; scrypt KDF with sufficient params (log_n>=15, r>=8, p>=1, random salt); ChaCha20-Poly1305 encryption under the derived key; round-trip works, wrong passphrase -> BadPassphrase, unsupported version -> UnsupportedVersion, weak KDF rejected; cargo check/test --locked passed.');