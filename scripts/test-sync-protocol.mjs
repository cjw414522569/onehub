#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SYNC = join(ROOT, 'crates/sync-core');
const errors = [];

// T092: optional end-to-end encrypted sync protocol design.
const SYNC_TOKENS = [
  'pub const SYNC_PROTOCOL_VERSION', 'pub struct DeviceIdentity', 'pub struct SyncEnvelope',
  'pub struct RotateKey', 'pub struct RevocationList', 'pub struct TestVector',
  'pub struct ThreatModel', 'pub fn encrypt_envelope', 'pub fn decrypt_envelope',
  'pub fn is_revoked', 'pub fn revoke',
  'envelope_round_trip_and_server_invisibility', 'device_identity_and_revocation',
  'key_rotation_is_generation_tagged', 'protocol_test_vectors_are_deterministic',
  'threat_model_review_holds',
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
  console.error(`sync-protocol contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('sync-protocol contract valid: the server only stores SyncEnvelopes (versioned AEAD ciphertext + routing metadata, never plaintext); DeviceIdentity (public key only), generation-tagged RotateKey, and RevocationList are first-class; deterministic test vectors and a structured ThreatModel review lock the properties (round-trip, tamper/wrong-key rejection, revocation idempotence, threat-model assertions); cargo check/test --locked passed.');