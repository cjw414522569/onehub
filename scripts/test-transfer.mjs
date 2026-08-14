#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/core-domain');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum TransferDirection', 'pub enum TransferMode', 'pub struct TransferSpec',
  'pub enum TransferStatus', 'pub struct TransferProgress', 'pub enum RemoteFileOp',
  'pub enum TransferError', 'pub const fn stable_code', 'pub fn fraction',
  'error_mapping_is_exhaustive_and_stable',
  'remote_file_ops_round_trip',
  'skip_and_resume_preserve_existing',
];
const FORBIDDEN_DEPENDENCIES = [
  'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui', 'windows-app-sdk',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'tokio', 'wgpu', 'harfbuzz',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/core-domain/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`core-domain has forbidden dependency: ${name}`);
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
  if (!sourceText.includes(token)) errors.push(`core-domain is missing required token: ${token}`);
}
for (const mode of ['Overwrite', 'SkipIfExists', 'Resume', 'AtomicReplace']) {
  if (!sourceText.includes(mode)) errors.push(`TransferMode is missing variant: ${mode}`);
}
for (const op of ['Stat', 'Mkdir', 'Rename', 'Delete', 'Chmod', 'Symlink', 'ReadLink']) {
  if (!sourceText.includes(op)) errors.push(`RemoteFileOp is missing variant: ${op}`);
}

for (const args of [['check', '-p', 'core-domain', '--locked'], ['test', '-p', 'core-domain', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p core-domain failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`transfer contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('transfer contract valid: progress/pause/resume/overwrite/atomic/checksum semantics, remote file ops, stable error mapping, cargo check/test --locked passed.');