#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/sftp-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct FileAttrs', 'pub fn encode_attrs', 'pub fn decode_attrs', 'pub fn mode_string',
  'pub struct SftpCapabilities', 'pub struct SftpClient', 'pub enum SftpStatus',
  'pub async fn init', 'pub async fn open', 'pub async fn read', 'pub async fn write',
  'pub async fn lstat', 'pub async fn stat', 'pub async fn fstat', 'pub async fn setstat',
  'pub async fn opendir', 'pub async fn readdir', 'pub async fn remove', 'pub async fn mkdir',
  'pub async fn rmdir', 'pub async fn realpath', 'pub async fn rename', 'pub async fn readlink',
  'pub async fn symlink', 'pub async fn extended', 'pub fn frame_packet', 'pub fn parse_packet',
  'pub struct VirtualFs', 'pub struct SftpServer', 'pub async fn serve', 'pub fn supports',
  'SSH_FXP_INIT', 'SSH_FXP_VERSION', 'SSH_FXP_STATUS', 'SSH_FXP_HANDLE', 'SSH_FXP_NAME',
  'init_probes_version_and_capabilities', 'mkdir_list_and_stat',
  'write_read_fstat_and_truncate', 'rename_delete_and_status_codes',
  'permissions_and_symlinks', 'realpath_and_nested_directories',
  'unsupported_extension_is_reported',
];
// In-house implementation: no external ssh-sftp crate.
const FORBIDDEN_DEPENDENCIES = ['ssh-sftp', 'russh-sftp', 'ssh2', 'libssh'];

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
  console.error(`sftp contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('sftp contract valid: in-house SFTP v3 client+server, capability probing, list/stat/mkdir/rename/delete/permissions/symlink integration over the wire, SSH_FX_* status codes, cargo check/test --locked passed.');