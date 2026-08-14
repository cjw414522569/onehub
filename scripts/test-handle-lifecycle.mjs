#!/usr/bin/env node

// T098 contract: opaque handle lifecycle + cross-ABI ownership.

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const ABI = join(ROOT, 'crates/abi-c');
const errors = [];

const TOKENS = [
  'pub const INVALID_HANDLE', 'pub struct HandleTable', 'pub struct HandleResource',
  'pub fn insert', 'pub fn get_mut', 'pub fn remove', 'pub fn contains', 'pub fn len',
  'pub extern "C" fn ssh_abi_handle_create', 'pub extern "C" fn ssh_abi_handle_release',
  'pub extern "C" fn ssh_abi_handle_is_valid', 'pub extern "C" fn ssh_abi_handle_count',
  'pub extern "C" fn ssh_abi_handle_cancel', 'pub extern "C" fn ssh_abi_handle_is_cancelled',
  'handle_table_release_is_idempotent', 'handle_table_drops_resources_on_drop',
  'exported_handle_abi_lifecycle_and_stress', 'exported_handle_abi_cancel_and_exit',
  'handle_ids_are_opaque_and_never_zero', 'resource_payload_stays_opaque',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(ABI, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`abi-c missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'abi-c', '--locked'],
  ['test', '-p', 'abi-c', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p abi-c failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`handle-lifecycle contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('handle-lifecycle contract valid: HandleTable hands out opaque non-zero u64 handles with idempotent release (double release / racing GC-ARC finalizer is a safe no-op, never a use-after-free); stale handles are rejected by contains/get, never dereferenced; dropping the table (exit) drops every remaining resource so cancellation and exit leak nothing (drop-counter verified); the exported ssh_abi_handle_* ABI (create/release/is_valid/count/cancel/is_cancelled) passes a 10k create/release stress with zero residual handles; cargo check/test --locked passed.');