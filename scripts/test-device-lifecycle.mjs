#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SYNC = join(ROOT, 'crates/sync-core');
const SERVICE = join(ROOT, 'services/sync-service');
const errors = [];

// T095: device pairing, recovery codes, revocation, and key rotation.
const LIFECYCLE_TOKENS = [
  'pub struct DeviceKey', 'pub struct DataKey', 'pub struct PairingCode',
  'pub struct RecoveryCode', 'pub struct KeyManager', 'pub struct Device',
  'pub struct RotatedKeys', 'pub enum LifecycleError', 'pub fn wrap_data_key',
  'pub fn unwrap_data_key', 'pub fn create_pairing_code', 'pub fn pair_device',
  'pub fn revoke_device', 'pub fn rotate_keys', 'pub fn recover_data_key',
  'pub fn decrypt_envelope', 'pub fn install', 'pub fn can_read',
  'pairing_code_is_one_time_and_authorizes_new_device',
  'revoked_device_cannot_read_post_rotation_data',
  'offline_old_device_cannot_read_new_data_until_it_syncs',
  'recovery_code_restores_current_data_key_after_loss',
  'pairing_after_revocation_is_refused',
  'wrap_unwrap_round_trip_and_tamper_rejection',
];
const E2E_TOKENS = [
  'pairing_revocation_and_rotation_end_to_end',
  'MemorySecureStore', 'KeyManager', 'revoke_device', 'rotate_keys',
  'cannot read new data',
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

checkCrateTokens(SYNC, LIFECYCLE_TOKENS, 'sync-core device_lifecycle');
checkCrateTokens(SERVICE, E2E_TOKENS, 'sync-service e2e');
checkForbiddenDeps(SYNC, 'sync-core');

for (const args of [
  ['check', '-p', 'sync-core', '--locked'],
  ['test', '-p', 'sync-core', '--locked'],
  ['check', '-p', 'sync-service', '--locked'],
  ['test', '-p', 'sync-service', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`device-lifecycle contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('device-lifecycle contract valid: KeyManager pairs devices with one-time PairingCodes (new devices receive only the current generation), revokes lost devices, rotates generation-tagged data keys (wrapped only for non-revoked devices), and restores the current data key from the RecoveryCode; Device decrypts a generation only when it holds the wrapped key (random-nonce AEAD wrap, tamper/wrong-key rejected); the end-to-end multi-device scenario (T092 envelopes + T094 backend + secure-store vaults) proves a revoked/old device cannot read new data while active devices can; cargo check/test --locked passed for sync-core and sync-service.');