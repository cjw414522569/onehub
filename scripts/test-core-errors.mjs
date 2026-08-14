#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/core-errors');
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
const REQUIRED_TOKENS = [
  'pub enum ErrorCode', 'pub enum Recoverability', 'pub enum RetrySuggestion',
  'pub enum MessageParam', 'pub struct ErrorInfo', 'impl From<DomainError> for ErrorInfo',
  'mapping_is_exhaustive_over_all_domain_variants',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/core-errors/Cargo.toml');
if (!existsSync(join(CRATE, 'src/lib.rs'))) errors.push('Missing crates/core-errors/src/lib.rs');

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
  if (/\{\s*path\s*=/.test(trimmed) && name !== 'core-domain') {
    errors.push(`core-errors must only path-depend on core-domain (found ${name})`);
  }
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`core-errors has forbidden runtime dependency: ${name}`);
}
for (const name of dependencyNames) {
  if (!['core-domain', 'serde'].includes(name)) errors.push(`core-errors runtime dependency is not approved: ${name}`);
}
if (!dependencyNames.includes('core-domain')) errors.push('core-errors must depend on core-domain');
if (!dependencyNames.includes('serde')) errors.push('core-errors must depend on serde (derive)');

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
    errors.push(`core-errors source references forbidden token: ${token}`);
  }
}
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`core-errors is missing required token: ${token}`);
}

for (const args of [['check', '-p', 'core-errors', '--locked'], ['test', '-p', 'core-errors', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8' });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p core-errors failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`core-errors contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('core-errors contract valid: stable codes, recoverability, retry, message params, exhaustive DomainError mapping, cargo check/test --locked passed.');