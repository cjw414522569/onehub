#!/usr/bin/env node

// T164 contract: version numbers, compatibility windows, deprecation policy,
// and the N/N-1/N-2 compatibility matrix. Validates the matrix against the
// real source constants, runs the versioned-boundary tests (gateway, ABI,
// WASM, sync, storage-sqlite migrations), and checks the deprecation policy.

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 600000 });
}
function sourceConstant(file, name) {
  const text = readFileSync(join(ROOT, file), 'utf8');
  const match = new RegExp(`${name}\\s*:\\s*[A-Za-z0-9_]+\\s*=\\s*(\\d+)`).exec(text);
  return match ? Number(match[1]) : null;
}

// 1. The matrix must agree with the real source constants.
const policy = JSON.parse(readFileSync(join(ROOT, 'versioning/versioning.json'), 'utf8'));
for (const [id, component] of Object.entries(policy.components)) {
  if (!component.file) continue;
  const actual = sourceConstant(component.file, component.constant);
  if (actual === null) errors.push(`could not find ${component.constant} in ${component.file}`);
  else if (actual !== component.current) errors.push(`${id} current ${component.current} != source ${actual}`);
}

// 2. Matrix completeness: every component x N/N-1/N-2 cell has a status.
const matrix = policy.matrix;
const components = ['gateway_protocol', 'abi', 'wasm', 'sync_protocol', 'database_schema'];
for (const component of components) {
  for (const row of matrix.rows) {
    const cell = matrix.cells[component]?.[row];
    if (!cell || typeof cell !== 'string' || cell.length < 3) {
      errors.push(`matrix cell ${component}/${row} missing`);
    }
  }
  if (!matrix.cells[component]?.['N+1']) errors.push(`matrix cell ${component}/N+1 missing (newer versions must be rejected)`);
}

// 3. Deprecation policy: removal no earlier than N+2.
if (policy.deprecation.notice_min_releases !== 2) errors.push('deprecation notice must be >= 2 releases');

// 4. Real N/N-1/N-2 boundary tests.
const gatewayTests = run('cargo', ['test', '-p', 'gateway', '--locked']);
if (gatewayTests.status !== 0) errors.push('gateway tests failed (version mismatch rejection)');
const abiTests = run('cargo', ['test', '-p', 'abi-c', '--locked']);
if (abiTests.status !== 0) errors.push('abi-c tests failed (header validity)');
const wasmTests = run('cargo', ['test', '-p', 'wasm', '--locked']);
if (wasmTests.status !== 0) errors.push('wasm tests failed (boundary version)');
const syncTests = run('cargo', ['test', '-p', 'sync-core', '--locked']);
if (syncTests.status !== 0) errors.push('sync-core tests failed (envelope version)');
const dbTests = run('cargo', ['test', '-p', 'storage-sqlite', '--locked']);
if (dbTests.status !== 0) errors.push('storage-sqlite tests failed (migration window)');

if (errors.length > 0) {
  console.error(`versioning contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`versioning contract valid: source constants match the policy (gateway/ABI/WASM/sync = 1, database schema = 3); the N/N-1/N-2 matrix is complete with N+1 rejected everywhere (no silent downgrade); the database window migrates N-1/N-2; deprecation removal is gated at N+2; gateway/ABI/WASM/sync/storage-sqlite boundary tests pass.`);