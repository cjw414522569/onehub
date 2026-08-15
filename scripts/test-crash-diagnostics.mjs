#!/usr/bin/env node

// T149 contract: crash capture, sanitization, retention, consent-gated
// upload. Runs the unit suite and the crash-trigger example, then audits the
// upload content: the canary and every content marker must be redacted, and
// the retention/deletion mechanism must be reported.

import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Build + unit tests (sanitizer, retention, deletion, upload consent).
const check = run('cargo', ['check', '-p', 'telemetry', '--locked']);
if (check.status !== 0) errors.push(`cargo check -p telemetry failed:\n${check.stdout}\n${check.stderr}`);
const test = run('cargo', ['test', '-p', 'telemetry', '--locked']);
if (test.status !== 0) errors.push(`cargo test -p telemetry failed:\n${test.stdout}\n${test.stderr}`);

// 2. Trigger a test crash and audit the upload content.
const trigger = run('cargo', ['run', '-p', 'telemetry', '--example', 'crash-trigger']);
if (trigger.status !== 0) errors.push(`crash-trigger example failed:\n${trigger.stdout}\n${trigger.stderr}`);
const combined = `${trigger.stdout}\n${trigger.stderr}`;

// Upload content audit: the canary and content markers must never appear.
const markers = ['CRASH_CANARY_b6d4e08f', 'db.internal', '10.0.0.5', 'c4145', 'ls -la'];
for (const marker of markers) {
  if (combined.includes(marker)) errors.push(`crash upload content leaked marker: ${marker}`);
}
if (!trigger.stdout.includes('crash:upload_consent=0')) errors.push('upload must be gated off by default');
if (!trigger.stdout.includes('crash:pruned=0')) errors.push('young dump must be retained');
if (!trigger.stdout.includes('crash:deleted=1')) errors.push('delete mechanism must remove the dump');

// The sanitized dump is valid JSON with schema_version 1 and redaction.
const dumpMatch = /crash:dump (\{.*\})/.exec(trigger.stdout);
if (!dumpMatch) {
  errors.push('crash:dump JSON missing');
} else {
  const dump = JSON.parse(dumpMatch[1]);
  if (dump.schema_version !== 1) errors.push('dump schema_version != 1');
  if (!String(dump.message).includes('[REDACTED]')) errors.push('dump message must be redacted');
}

if (errors.length > 0) {
  console.error(`crash-diagnostics contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('crash-diagnostics contract valid: crash dumps are captured and sanitized (canary/host/command markers redacted), retention and deletion mechanisms are defined and reported (pruned=0, deleted=1), and upload is gated behind explicit consent (consent=0 by default); the upload content audit found zero leaks.');