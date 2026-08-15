#!/usr/bin/env node

// T146 contract: structured logging, trace ids, dynamic level control.
// Runs the unit suite plus a canary secret log scan: the canary example
// attempts to log a secret under sensitive field names and the contract
// scans every emitted byte — the canary must never appear.

import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CANARY = 'CANARY_SECRET_5e8c2f91a4b7';
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Build + unit tests (structured log, trace ids, dynamic levels,
//    sensitive-field policy).
const check = run('cargo', ['check', '-p', 'telemetry', '--locked']);
if (check.status !== 0) errors.push(`cargo check -p telemetry failed:\n${check.stdout}\n${check.stderr}`);
const test = run('cargo', ['test', '-p', 'telemetry', '--locked']);
if (test.status !== 0) errors.push(`cargo test -p telemetry failed:\n${test.stdout}\n${test.stderr}`);

// 2. Canary secret log scan: run the canary example and scan ALL emitted
//    output (stdout + stderr) for the canary value.
const canary = run('cargo', ['run', '-p', 'telemetry', '--example', 'canary']);
if (canary.status !== 0) errors.push(`canary example failed:\n${canary.stdout}\n${canary.stderr}`);
const combined = `${canary.stdout}\n${canary.stderr}`;
if (combined.includes(CANARY)) errors.push('canary secret leaked into emitted log output');
if (!canary.stdout.includes('canary_scan_complete')) errors.push('canary example did not emit the benign completion line');
if (!/level=info trace=[0-9a-f]{16} target=gateway\.canary message=operation/.test(canary.stdout)) {
  errors.push('structured log line shape missing (level/trace/target/message)');
}

// 3. Also scan the test run output for the canary.
if (`${test.stdout}\n${test.stderr}`.includes(CANARY)) errors.push('canary leaked into test output');

if (errors.length > 0) {
  console.error(`telemetry-logging contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('telemetry-logging contract valid: structured logs carry level/trace/target/message with trace-id correlation and dynamic level control; the sensitive-field denylist drops token/password/secret/key/host/terminal fields so default logs have no sensitive fields; the canary secret scan found zero leaks in emitted log output.');