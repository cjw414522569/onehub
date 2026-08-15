#!/usr/bin/env node

// T155 contract: resource-leak soak (memory/thread/handle/socket/GPU).
// Runs the 10k-cycle soak three times, asserts byte-identical output with
// zero leaks and zero thread delta, and regenerates
// fault-injection/resource-leak-soak.report.json (byte-identical). The
// sanitizer (ASan) is blocked_unavailable_toolchain on this stable-only
// host (requires nightly -Zsanitizer); the soak is the CI gate here.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 600000 });
}

// 1. Run the 10k-cycle soak three times; every run must be byte-identical.
const outputs = [];
for (let i = 0; i < 3; i += 1) {
  const res = run('cargo', ['run', '-p', 'gateway', '--example', 'resource-soak']);
  if (res.status !== 0) errors.push(`resource-soak run ${i} failed:\n${res.stdout}\n${res.stderr}`);
  outputs.push(res.stdout);
}
if (new Set(outputs).size !== 1) errors.push('resource-soak output differs across runs (flaky)');
if (!outputs[0].includes('RESOURCE_SOAK cycles=10000 leaks=0 thread_delta=0 stable=true')) {
  errors.push('resource soak did not report 10k cycles with zero leaks');
}
for (const resource of ['connections', 'windows', 'handles', 'transfers', 'frames']) {
  if (!outputs[0].includes(`RESOURCE ${resource}_live=0`)) errors.push(`resource ${resource} did not return to baseline`);
}

// 2. Regenerate the report (deterministic) and compare byte-identical.
const lines = outputs[0].trim().split('\n');
const report = {
  schema_version: 1,
  cycles: 10000,
  leaks: 0,
  thread_delta: 0,
  stable: true,
  runs_verified: 3,
  sanitizer: {
    status: 'blocked_unavailable_toolchain',
    reason: 'stable-only toolchain; -Zsanitizer=address requires nightly',
    attempt: 'RUSTFLAGS=-Zsanitizer=address cargo check -> "error: 1 nightly option were parsed"',
  },
  resources: lines.filter((l) => l.startsWith('RESOURCE ')).map((l) => l.slice('RESOURCE '.length)),
};
const reportText = `${JSON.stringify(report, null, 2)}\n`;
const reportPath = join(ROOT, 'fault-injection/resource-leak-soak.report.json');
if (process.argv.includes('--write')) {
  mkdirSync(dirname(reportPath), { recursive: true });
  writeFileSync(reportPath, reportText, 'utf8');
  console.log(`wrote ${reportPath}`);
} else if (existsSync(reportPath)) {
  if (!readFileSync(reportPath).equals(Buffer.from(reportText, 'utf8'))) {
    errors.push('resource-leak-soak.report.json changed (regenerate with --write)');
  }
} else {
  errors.push('resource-leak-soak.report.json missing (regenerate with --write)');
}

if (errors.length > 0) {
  console.error(`resource-soak contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('resource-soak contract valid: 10,000 connect/disconnect/window/handle/transfer/GPU-frame cycles leave zero leaked connections, windows, handles, transfers, or frames and zero thread delta; three repeated runs are byte-identical; the sanitizer is documented blocked_unavailable_toolchain (nightly required).');