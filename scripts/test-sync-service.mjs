#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SERVICE = join(ROOT, 'services/sync-service');
const errors = [];

// T094: minimal trusted sync backend (ciphertext-only, quota + rate limit,
// content-free audit, API contract / authorization / load tests).
const SERVICE_TOKENS = [
  'pub const MODULE_ID', 'pub struct ServiceConfig', 'pub struct SyncBackend',
  'pub enum ServiceError', 'pub struct AuditRecord', 'pub struct EnvelopeMeta',
  'pub trait Clock', 'pub fn put', 'pub fn get', 'pub fn delete', 'pub fn list',
  'pub fn usage', 'pub fn audit_log', 'QuotaExceeded', 'RateLimited', 'Forbidden',
  'UnsupportedVersion', 'mailbox_key', 'encode_envelope',
  'api_contract_put_get_list_delete_round_trip',
  'unauthorized_device_cannot_read_or_write', 'quota_is_enforced_and_frees_on_delete',
  'rate_limit_denies_bursts_and_recovers', 'audit_log_is_content_free',
  'concurrent_load_preserves_all_writes_and_quota',
  'same_device_concurrent_puts_do_not_lose_writes',
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

if (!existsSync(join(SERVICE, 'Cargo.toml'))) errors.push('Missing services/sync-service/Cargo.toml');
checkCrateTokens(SERVICE, SERVICE_TOKENS, 'sync-service');
checkForbiddenDeps(SERVICE, 'sync-service');

for (const args of [
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
  console.error(`sync-service contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('sync-service contract valid: SyncBackend stores only SyncEnvelopes (ciphertext + routing metadata, never plaintext) in sender/recipient mailboxes; per-device quota (QuotaExceeded) and token-bucket rate limit (RateLimited with retry_after_secs) are enforced; the audit log (AuditRecord) is content-free (device/envelope-id/action/byte_len/timestamp only); authorization refuses forged senders (Forbidden) and isolates unauthorized devices; API contract, authorization (越权), and concurrent load tests pass; cargo check/test --locked passed.');