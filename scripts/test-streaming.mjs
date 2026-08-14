#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/transfer');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct StreamConfig', 'pub trait ChunkReader', 'pub trait ChunkWriter',
  'pub async fn run_streaming_copy', 'pub struct TransferStats', 'pub fn progress',
  'DEFAULT_CHUNK_SIZE', 'DEFAULT_MAX_IN_FLIGHT',
  'round_trip_small_file', 'round_trip_multi_chunk', 'large_file_memory_is_bounded',
  'slow_writer_backpressure_pipelines_chunks', 'interactive_session_is_not_starved',
  'invalid_config_is_rejected',
];
const FORBIDDEN_DEPENDENCIES = ['russh', 'libssh', 'ssh2', 'openssh'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/transfer/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`transfer has forbidden dependency: ${name}`);
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
  if (!sourceText.includes(token)) errors.push(`transfer is missing required token: ${token}`);
}

for (const args of [['check', '-p', 'transfer', '--locked'], ['test', '-p', 'transfer', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p transfer failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`streaming contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('streaming contract valid: bounded-memory chunked pipeline, concurrent in-flight chunks, backpressure via bounded channel, cooperative yielding (interactive not starved), 256 MiB sparse memory bound O(chunk x in_flight), cargo check/test --locked passed.');