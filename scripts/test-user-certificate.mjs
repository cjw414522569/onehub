#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum UserCertVerification', 'pub fn ed25519_ca_fingerprint',
  'pub fn verify_user_certificate', 'pub fn is_valid',
  'trusted_ca_valid_certificate_is_accepted',
  'expired_certificate_is_diagnosed',
  'untrusted_ca_is_diagnosed',
  'principal_mismatch_is_diagnosed',
];
const FORBIDDEN_DEPENDENCIES = [
  'russh', 'russh-keys', 'libssh', 'libssh2', 'ssh2', 'ssh2-sys', 'openssh',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/ssh-backend/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`ssh-backend has forbidden dependency: ${name}`);
}

const sourceFiles = [];
function collect(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collect(absolute);
    else if (entry.name.endsWith('.rs')) sourceFiles.push(absolute);
  }
}
if (existsSync(join(CRATE, 'src'))) collect(join(CRATE, 'src'));
const sourceText = sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`ssh-backend is missing required token: ${token}`);
}
for (const variant of ['Valid', 'Malformed', 'NotYetValid', 'Expired', 'SignatureInvalid', 'UntrustedCa', 'PrincipalMismatch']) {
  if (!sourceText.includes(variant)) errors.push(`UserCertVerification is missing variant: ${variant}`);
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`user certificate contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('user certificate contract valid: positive + negative (expired / not-yet-valid / untrusted CA / principal mismatch / malformed) integration tests, cargo check/test --locked passed.');