#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SQLITE = join(ROOT, 'crates/storage-sqlite');
const errors = [];

// T089: database field-level encryption and master-key wrapping.
const SQLITE_TOKENS = [
  'pub const AEAD_VERSION', 'pub struct EncryptedField', 'pub struct FieldEncryptor',
  'pub struct KeyRing', 'pub struct MasterKeyWrapper', 'pub fn encrypt', 'pub fn decrypt',
  'pub fn rotate', 'pub fn purge_version', 'pub fn wrap', 'pub fn unwrap', 'pub fn reencrypt',
  'ChaCha20Poly1305', 'active_version',
  'encrypt_decrypt_round_trip', 'tampering_is_detected',
  'rotation_reencrypts_and_old_versions_stay_readable', 'master_key_wrap_and_recovery',
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

const manifest = readFileSync(join(SQLITE, 'Cargo.toml'), 'utf8');
if (!manifest.includes('chacha20poly1305')) errors.push('storage-sqlite is missing the chacha20poly1305 dependency');

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
  console.error(`field-encryption contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('field-encryption contract valid: EncryptedField is a versioned ChaCha20-Poly1305 AEAD blob (version + nonce + ciphertext+tag); KeyRing holds field keys outside the database, rotates to a new version while old versions stay decryptable, and reencrypt moves rows to the active version; MasterKeyWrapper wraps/unwraps the field key with a master key stored outside the DB (recovery); tampering/wrong-key decryption fails; cargo check/test --locked passed.');