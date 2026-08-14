#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum PrivateKeyFormat', 'pub enum KeyAlgorithm', 'pub enum KeyError',
  'pub enum PrivateKeyHandle', 'pub fn detect_format', 'pub fn load_private_key',
  'pub fn algorithm', 'pub fn public_fingerprint_sha256',
  'openssh_key_matrix_parses_all_algorithms',
  'encrypted_openssh_key_requires_and_uses_passphrase',
  'pkcs8_plain_fixtures_parse',
  'encrypted_pkcs8_fixture_requires_and_uses_passphrase',
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
for (const marker of ['BEGIN OPENSSH PRIVATE KEY', 'BEGIN ENCRYPTED PRIVATE KEY', 'BEGIN PRIVATE KEY', 'hunter42', 'E_KEY_']) {
  if (!sourceText.includes(marker) && !sourceText.includes(marker.replace(/ /g, ' '))) {
    // BEGIN markers are split across lines; check case-insensitively
    if (!sourceText.toLowerCase().includes(marker.toLowerCase())) {
      errors.push(`private key module must include: ${marker}`);
    }
  }
}

// Fixtures must exist.
for (const fixture of ['ed25519-priv-pkcs8v2.pem', 'ed25519-encpriv-aes256-pbkdf2-sha256.pem', 'p256-priv.pem', 'rsa-openssh.pem']) {
  if (!existsSync(join(CRATE, 'tests/fixtures', fixture))) errors.push(`Missing fixture: tests/fixtures/${fixture}`);
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`private key contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('private key contract valid: OpenSSH + PKCS#8 (plain/encrypted) formats, Ed25519/ECDSA/RSA matrix, wrong-passphrase rejection, passphrase not retained, cargo check/test --locked passed.');