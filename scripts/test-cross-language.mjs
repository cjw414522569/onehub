#!/usr/bin/env node

// T152 contract: cross-language integration harness + deterministic fake
// server. Runs the abi-c FFI harness (100 consecutive runs, byte-identical)
// and the gateway fake-server example three times, asserting every run is
// byte-identical (no flakiness) and the FFI / cancel / error / lifecycle /
// isolation outcomes are exactly as expected.

import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 600000 });
}

// 1. Build + the FFI harness (runs the scenario 100x internally).
const check = run('cargo', ['check', '-p', 'gateway', '-p', 'abi-c', '--locked']);
if (check.status !== 0) errors.push(`cargo check failed:\n${check.stdout}\n${check.stderr}`);
const ffi = run('cargo', ['test', '-p', 'abi-c', '--test', 'ffi_harness', '--locked']);
if (ffi.status !== 0) errors.push(`ffi_harness failed:\n${ffi.stdout}\n${ffi.stderr}`);

// 2. The deterministic fake gateway server: run it three times and require
//    byte-identical output (no flakiness) with the expected outcomes.
const outputs = [];
for (let i = 0; i < 3; i += 1) {
  const res = run('cargo', ['run', '-p', 'gateway', '--example', 'fake-server']);
  if (res.status !== 0) errors.push(`fake-server run ${i} failed:\n${res.stdout}\n${res.stderr}`);
  outputs.push(res.stdout);
}
if (new Set(outputs).size !== 1) errors.push('fake-server output differs across runs (flaky)');

const expected = [
  'HARNESS runs=100 stable=true',
  'HARNESS hello=accepted',
  'HARNESS data_before_auth=NotAuthenticated',
  'HARNESS bad_token=NotAuthenticated',
  'HARNESS version_mismatch=VersionMismatch',
  'HARNESS bad_resume=InvalidResume',
  'HARNESS private_target=PrivateAddress',
  'HARNESS phase_after_close=Closed',
  'HARNESS isolation=TenantIsolationViolation',
  'HARNESS tenant_ok=Ok("ok")',
];
for (const line of expected) {
  if (!outputs[0].includes(line)) errors.push(`fake-server missing expected outcome: ${line}`);
}

if (errors.length > 0) {
  console.error(`cross-language contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('cross-language contract valid: the abi-c FFI harness (version/header/handle/event-stream cancel+error+lifecycle) is stable across 100 consecutive runs; the deterministic fake gateway server reproduces FFI/cancel/error/lifecycle/isolation outcomes byte-identically across repeated runs with zero flakiness.');