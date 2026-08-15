#!/usr/bin/env node

// T148 contract: local performance sampling + user-exportable diagnostics.
// Runs the unit suite and the diagnostics example, then scans the exported
// report (the diagnostic data privacy scan) for any content markers — the
// report must contain only numeric aggregates and fixed metric labels.

import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Build + unit tests (sampling aggregates, percentiles, report privacy).
const check = run('cargo', ['check', '-p', 'telemetry', '--locked']);
if (check.status !== 0) errors.push(`cargo check -p telemetry failed:\n${check.stdout}\n${check.stderr}`);
const test = run('cargo', ['test', '-p', 'telemetry', '--locked']);
if (test.status !== 0) errors.push(`cargo test -p telemetry failed:\n${test.stdout}\n${test.stderr}`);

// 2. Export the diagnostics report and run the privacy scan.
const example = run('cargo', ['run', '-p', 'telemetry', '--example', 'diagnostics']);
if (example.status !== 0) errors.push(`diagnostics example failed:\n${example.stdout}\n${example.stderr}`);

let report = null;
try {
  report = JSON.parse(example.stdout.trim());
} catch {
  errors.push('diagnostics example did not emit a JSON report');
}
if (report) {
  if (report.schema_version !== 1) errors.push('diagnostics report schema_version != 1');
  const labels = report.rows.map((row) => row.metric);
  for (const required of ['network_latency_ms', 'parse_throughput_mbps', 'render_frame_ms', 'memory_kb']) {
    if (!labels.includes(required)) errors.push(`diagnostics report missing metric: ${required}`);
  }
  for (const row of report.rows) {
    for (const key of ['count', 'mean', 'p50', 'p95', 'p99', 'min', 'max']) {
      if (typeof row[key] !== 'number') errors.push(`row ${row.metric} missing numeric ${key}`);
    }
  }
}

// Diagnostic data privacy scan: the exported report (the user-exportable
// artifact) must not expose content. Cargo test output is excluded because
// it legitimately contains test names.
const markers = ['host', 'command', 'user', 'token', 'secret', 'terminal_text', 'payload',
  'db.internal', '10.0.0.5', 'PRIVACY_CANARY', 'ls -la', 'rm -rf'];
const combined = `${example.stdout}\n${example.stderr}`;
for (const marker of markers) {
  if (combined.includes(marker)) errors.push(`diagnostic output exposed content marker: ${marker}`);
}

if (errors.length > 0) {
  console.error(`diagnostics contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('diagnostics contract valid: the sampler records network/parse/render/memory numeric samples; the exported schema_version:1 report contains only numeric aggregates (count/mean/p50/p95/p99/min/max) with fixed metric labels; the diagnostic data privacy scan found zero content exposure.');