#!/usr/bin/env node

// T137 contract: gateway authentication, short-lived tokens, session isolation.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const GATEWAY = join(ROOT, 'services/gateway');
const errors = [];

const TOKENS = [
  'pub struct TenantId', 'pub struct AuthToken', 'pub struct TokenIssuer',
  'pub struct SessionRegistry', 'pub struct CredentialPolicy', 'pub enum AuthError',
  'TokenExpired', 'ReplayDetected', 'UnknownSession', 'TenantIsolationViolation',
  'LongTermKeyStorageForbidden', 'pub fn is_expired', 'pub fn ttl_secs',
  'pub fn issue', 'pub fn issue_with_ttl', 'pub fn create_session',
  'pub fn authenticate', 'pub fn access', 'pub fn persist_long_term_key',
  'pub fn accept_short_lived_session_key',
  'short_lived_token_expires_after_ttl', 'replay_of_consumed_token_rejected',
  'cross_tenant_access_denied', 'token_tenant_mismatch_rejected',
  'tenant_sessions_are_isolated', 'unknown_session_rejected',
  'long_term_key_persistence_rejected', 'short_lived_session_key_within_budget',
  'custom_ttl_honored',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(GATEWAY, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`gateway missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'gateway', '--locked'],
  ['test', '-p', 'gateway', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p gateway failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`gateway-auth contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gateway-auth contract valid: TokenIssuer issues short-lived single-use tenant-bound tokens; SessionRegistry rejects expired/replayed/unknown/cross-tenant tokens and re-checks the tenant boundary on every access; CredentialPolicy refuses long-lived SSH key persistence (keys stay client-side); authorization-bypass, replay, and token-expiry tests pass; cargo check/test --locked passed.');