#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/core-domain');
const errors = [];

const FORBIDDEN_DEPENDENCIES = [
  'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui', 'windows-app-sdk',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'tokio', 'wgpu', 'harfbuzz',
];
const FORBIDDEN_IMPORT_TOKENS = [
  'winui', 'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'webview', 'russh', 'libssh', 'sqlx', 'sqlite', 'storage-sqlite', 'secure-store',
];
const REQUIRED_MODEL_TOKENS = [
  'pub struct HostId', 'pub struct HostAddress', 'pub struct Host',
  'pub enum CredentialKind', 'pub struct CredentialRef',
  'pub struct SessionProfile', 'pub enum DomainError',
  'DEFAULT_SSH_PORT',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/core-domain/Cargo.toml');
if (!existsSync(join(CRATE, 'src/lib.rs'))) errors.push('Missing crates/core-domain/src/lib.rs');

// 1. Manifest dependency policy.
const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
const dependencyNames = [];
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  dependencyNames.push(name);
  if (/\{\s*path\s*=/.test(trimmed)) errors.push(`core-domain must not have path dependencies (found ${name})`);
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`core-domain has forbidden runtime dependency: ${name}`);
}
const allowed = new Set(['serde']);
for (const name of dependencyNames) {
  if (!allowed.has(name)) errors.push(`core-domain runtime dependency is not approved: ${name}`);
}
if (!dependencyNames.includes('serde')) errors.push('core-domain must depend on serde (derive) for serialization');

// 2. Source import policy.
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
for (const token of FORBIDDEN_IMPORT_TOKENS) {
  if (new RegExp(`\\b${token}\\b`, 'i').test(sourceText)) {
    errors.push(`core-domain source references forbidden token: ${token}`);
  }
}

// 3. Model surface.
const libText = readFileSync(join(CRATE, 'src/lib.rs'), 'utf8');
const allText = libText + '\n' + sourceText;
for (const token of REQUIRED_MODEL_TOKENS) {
  if (!allText.includes(token)) errors.push(`core-domain is missing required model token: ${token}`);
}
for (const token of ['use serde::', 'Deserialize', 'Serialize']) {
  if (!allText.includes(token)) errors.push(`core-domain source does not use serde: ${token}`);
}

// 4. Cargo checks.
for (const args of [['check', '-p', 'core-domain', '--locked'], ['test', '-p', 'core-domain', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8' });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p core-domain failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`core-domain contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`core-domain contract valid: models present, serde-only dependencies, no UI/db/ssh types, cargo check/test --locked passed (models: ${REQUIRED_MODEL_TOKENS.length}).`);