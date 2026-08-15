#!/usr/bin/env node

// T154 contract: network fault injection matrix. Runs the gateway fault
// matrix three times, asserts byte-identical output (no flaky), all six
// fault kinds pass (latency/packet-loss/reordering/disconnect/DNS-failure/
// network-switch), and regenerates fault-injection/fault-matrix.report.json
// (byte-identical).

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];
const kinds = ['latency', 'packet_loss', 'reordering', 'disconnect', 'dns_failure', 'network_switch'];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 600000 });
}

// 1. Run the matrix three times; every run must be byte-identical.
const outputs = [];
for (let i = 0; i < 3; i += 1) {
  const res = run('cargo', ['run', '-p', 'gateway', '--example', 'fault-matrix']);
  if (res.status !== 0) errors.push(`fault-matrix run ${i} failed:\n${res.stdout}\n${res.stderr}`);
  outputs.push(res.stdout);
}
if (new Set(outputs).size !== 1) errors.push('fault-matrix output differs across runs (flaky)');
if (!outputs[0].includes('FAULT_MATRIX covered=6 passed=6 stable=true')) {
  errors.push('fault matrix did not report 6/6 pass + stable');
}
for (const kind of kinds) {
  if (!outputs[0].includes(`FAULT ${kind}=true`)) errors.push(`fault kind not covered/passed: ${kind}`);
}

// 2. Regenerate the report (deterministic) and compare byte-identical.
const reportLines = outputs[0].trim().split('\n');
const report = {
  schema_version: 1,
  covered: kinds,
  passed: kinds.length,
  stable: true,
  runs_verified: 3,
  results: reportLines.filter((l) => l.startsWith('FAULT ')).map((l) => l.slice('FAULT '.length)),
};
const reportText = `${JSON.stringify(report, null, 2)}\n`;
const reportPath = join(ROOT, 'fault-injection/fault-matrix.report.json');
if (process.argv.includes('--write')) {
  mkdirSync(dirname(reportPath), { recursive: true });
  writeFileSync(reportPath, reportText, 'utf8');
  console.log(`wrote ${reportPath}`);
} else if (existsSync(reportPath)) {
  if (!readFileSync(reportPath).equals(Buffer.from(reportText, 'utf8'))) {
    errors.push('fault-matrix.report.json changed (regenerate with --write)');
  }
} else {
  errors.push('fault-matrix.report.json missing (regenerate with --write)');
}

if (errors.length > 0) {
  console.error(`fault-matrix contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('fault-matrix contract valid: latency, packet loss, reordering, disconnect, DNS failure, and network switching are all covered and pass deterministically; three repeated runs are byte-identical (no flaky); fault-matrix.report.json regenerates byte-identical.');