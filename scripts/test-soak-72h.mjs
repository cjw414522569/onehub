#!/usr/bin/env node

// T163 contract: 72-hour multi-session soak + network fluctuation.
// Runs the compressed 72-hour soak (14,400 cycles with interleaved network
// fluctuations) three times, asserting byte-identical output, no crash, no
// deadlock, no unbounded growth, and no secret leakage (canary scan), and
// archives the resource curve. With --write, writes the reports.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];
const CANARY = 'SOAK72_CANARY_1f4e9a27';

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 600000 });
}

const outputs = [];
for (let i = 0; i < 3; i += 1) {
  const res = run('cargo', ['run', '-p', 'gateway', '--example', 'soak-72h']);
  if (res.status !== 0) errors.push(`soak-72h run ${i} failed (crash/exit ${res.status}):\n${res.stdout}\n${res.stderr}`);
  outputs.push(res.stdout);
}
// The only run-to-run variable is the wall-clock `elapsed_secs`; the
// resource curve and invariants must be byte-identical.
function normalize(out) {
  const m = /SOAK72_METRICS (\{.*\})/.exec(out);
  if (!m) return out;
  const data = JSON.parse(m[1]);
  delete data.soak.elapsed_secs;
  return JSON.stringify(data);
}
if (new Set(outputs.map(normalize)).size !== 1) errors.push('soak-72h output differs across runs (flaky/deadlock)');
const combined = outputs[0] ?? '';
if (combined.includes(CANARY)) errors.push('canary secret leaked into soak output');

const match = /SOAK72_METRICS (\{.*\})/.exec(combined);
if (!match) errors.push('soak-72h did not emit SOAK72_METRICS');
const soak = match ? JSON.parse(match[1]).soak : {};
const inv = soak.invariants ?? {};
if (inv.no_crash !== true) errors.push('soak reported a crash');
if (inv.no_deadlock !== true) errors.push('soak reported a deadlock');
if (inv.open_sessions_final !== 0) errors.push('open sessions did not return to baseline');
if (inv.consumed_tokens_final > 4096) errors.push('consumed-token window unbounded');
if (inv.live_connections_final !== 0) errors.push('live connections leaked');
if ((soak.resource_curve ?? []).length !== 72) errors.push('resource curve missing 72 hourly samples');

if (errors.length > 0) {
  console.error(`soak-72h contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

// Archive: the resource curve (per T003: artifacts/perf/<platform>/<commit>/)
// and the report.
if (process.argv.includes('--write')) {
  const curvePath = join(ROOT, 'artifacts/perf/gateway/soak72h/resource-curve.json');
  mkdirSync(dirname(curvePath), { recursive: true });
  writeFileSync(curvePath, `${JSON.stringify(soak.resource_curve, null, 2)}\n`, 'utf8');
  const reportPath = join(ROOT, 'docs/reports/SOAK_72H_T163.json');
  writeFileSync(reportPath, `${JSON.stringify({
    task: 'T163', status: 'pass',
    verified_at_utc: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    soak: { simulated_hours: soak.simulated_hours, cycles: soak.cycles, elapsed_secs: soak.elapsed_secs, invariants: inv },
    secret_leak_scan: 'pass',
    resource_curve: `artifacts/perf/gateway/soak72h/resource-curve.json`,
  }, null, 2)}\n`, 'utf8');
  console.log(`wrote ${curvePath} and ${reportPath}`);
}

console.log(`soak-72h contract valid: ${soak.simulated_hours} simulated hours, ${soak.cycles} cycles in ${soak.elapsed_secs}s; no crash, no deadlock, open_sessions=0, consumed_tokens=${inv.consumed_tokens_final} (bounded), live_connections=0; canary secret scan zero leaks; 3 runs byte-identical; resource curve archived.`);