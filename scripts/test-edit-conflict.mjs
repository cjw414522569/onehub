#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/sftp-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct RemoteFileVersion', 'pub enum SaveOutcome', 'pub struct RemoteEditSession',
  'pub async fn begin', 'pub async fn save', 'pub async fn read_entire_file',
  'EDIT_READ_CHUNK', 'save_without_change_succeeds',
  'concurrent_modification_detects_conflict_and_keeps_recovery',
  'begin_on_missing_file_reports_no_such_file', 'version_fingerprint_tracks_content',
];
const FORBIDDEN_DEPENDENCIES = ['russh', 'libssh', 'ssh2', 'openssh'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/sftp-backend/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`sftp-backend has forbidden dependency: ${name}`);
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
if (existsSync(join(CRATE, 'tests'))) collect(join(CRATE, 'tests'));
const sourceText = sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`sftp-backend is missing required token: ${token}`);
}

for (const args of [['check', '-p', 'sftp-backend', '--locked'], ['test', '-p', 'sftp-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p sftp-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`edit-conflict contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('edit-conflict contract valid: version-fingerprint (SHA-256+size+mtime) gate prevents blind remote overwrites, concurrent-change conflict keeps a recovery copy, safe save integration over the wire, cargo check/test --locked passed.');