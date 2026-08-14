#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/secret');
const errors = [];

const FORBIDDEN_DEPENDENCIES = [
  'serde', 'serde_json', 'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'tokio', 'wgpu', 'harfbuzz',
];
const REQUIRED_TOKENS = [
  'pub struct SecretBytes', 'pub struct SecretString', 'impl Drop for SecretBytes',
  'impl Drop for SecretString', 'pub fn expose_secret', 'compile_fail',
  'clear_buffer', 'std::hint::black_box', 'zeroize',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/secret/Cargo.toml');

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
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`secret has forbidden dependency: ${name}`);
}
for (const name of dependencyNames) {
  if (!['zeroize'].includes(name)) errors.push(`secret runtime dependency is not approved: ${name}`);
}
if (!dependencyNames.includes('zeroize')) errors.push('secret must depend on zeroize');

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
  if (!sourceText.includes(token)) errors.push(`secret is missing required token: ${token}`);
}
// The secret types must NOT derive or implement leaking traits.
const secretStructLines = sourceText.split(/\r?\n/).filter((line) => /pub struct Secret(Bytes|String)/.test(line));
for (const line of secretStructLines) {
  const leading = sourceText.split(line)[0];
  const before = leading.slice(-400);
  if (/#\[derive/.test(before)) errors.push(`Secret struct must not derive traits:\n${before}`);
}

for (const args of [['check', '-p', 'secret', '--locked'], ['test', '-p', 'secret', '--locked'], ['test', '-p', 'secret', '--locked', '--doc']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args.join(' ')} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`secret contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('secret contract valid: zeroize-only dependency, no leaking derives, auto-zero drop path, compile_fail doctests, cargo check/test/--doc --locked passed.');