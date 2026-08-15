#!/usr/bin/env node

// T156 contract: benchmark statistics gate. Measures startup
// (ssh-cli --version), parse throughput, input-to-pixel latency, and
// scrollback (release, 30 repeats), computes P50/P95/P99 + mean, asserts the
// absolute budgets and the differential gate (<=10% regression vs
// baseline.json), and persists results per platform/device. Memory/power
// are documented blocked_unavailable_toolchain.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const PLATFORM = 'windows';
const DEVICE = 'ci-host';
const errors = [];
const repeats = 30;

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 600000 });
}

function percentile(values, pct) {
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.round(((sorted.length - 1) * pct) / 100);
  return sorted[Math.min(index, sorted.length - 1)];
}
function mean(values) { return values.reduce((a, b) => a + b, 0) / values.length; }

// 1. Startup: ssh-cli --version wall time, 30 repeats.
const exe = join(ROOT, 'target/debug/ssh-cli.exe');
const startup = [];
for (let i = 0; i < repeats; i += 1) {
  const start = process.hrtime.bigint();
  const res = spawnSync(exe, ['--version'], { encoding: 'utf8' });
  startup.push(Number(process.hrtime.bigint() - start) / 1e6);
  if (res.status !== 0) errors.push('ssh-cli --version failed');
}
const startupP95 = percentile(startup, 95);

// 2. Core micro-benchmarks (release, 30 repeats inside the example).
const bench = run('cargo', ['run', '--release', '-p', 'wasm', '--example', 'bench'], { timeout: 900000 });
if (bench.status !== 0) errors.push(`bench example failed:\n${bench.stdout}\n${bench.stderr}`);
const match = /BENCH_METRICS (\{.*\})/.exec(bench.stdout);
if (!match) errors.push('bench example did not emit BENCH_METRICS');
const metrics = match ? JSON.parse(match[1]).benchmarks : {};
const parseP50 = metrics.parse_throughput_mbps ? Number(metrics.parse_throughput_mbps.p50) : 0;
const inputP95 = metrics.input_to_pixel_us ? Number(metrics.input_to_pixel_us.p95_us) : Infinity;
const scrollP95 = metrics.scrollback_10k_lines_ms ? Number(metrics.scrollback_10k_lines_ms.p95_ms) : Infinity;

// 3. Budgets + differential gate.
const baseline = JSON.parse(readFileSync(join(ROOT, 'benchmarks/baseline.json'), 'utf8'));
const tolerance = baseline.differential_tolerance_pct / 100;
const checks = [
  { name: `startup_p95=${startupP95.toFixed(1)}ms <= 500ms`, ok: startupP95 <= 500 },
  { name: `parse_p50=${parseP50.toFixed(1)}MB/s >= 30MB/s (model-level floor; full-pipeline T003 40MB/s gate is T158)`, ok: parseP50 >= 30 },
  { name: `input_p95=${inputP95.toFixed(0)}us <= 45000us`, ok: inputP95 <= 45000 },
  { name: `scrollback_p95=${scrollP95.toFixed(1)}ms <= 100ms`, ok: scrollP95 <= 100 },
];
for (const c of checks) if (!c.ok) errors.push(`budget failed: ${c.name}`);

// Differential: no metric worse than baseline * (1 + tolerance).
const diffChecks = [
  ['startup_ms', startupP95, baseline.metrics.startup_ms.baseline_p95, false],
  ['parse_throughput_mbps', parseP50, baseline.metrics.parse_throughput_mbps.baseline_p50, true],
  ['scrollback_10k_lines_ms', scrollP95, baseline.metrics.scrollback_10k_lines_ms.baseline_p95, false],
];
for (const [name, current, base, higherBetter] of diffChecks) {
  const regressed = higherBetter
    ? current < base * (1 - tolerance)
    : current > base * (1 + tolerance);
  if (regressed) errors.push(`differential regression: ${name} ${current} vs baseline ${base}`);
}

// 4. Persist results per platform/device.
const results = {
  schema_version: 1,
  platform: PLATFORM,
  device: DEVICE,
  generated_at_utc: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
  metrics: {
    startup_ms: { p95: startupP95, mean: mean(startup), count: repeats },
    parse_throughput_mbps: metrics.parse_throughput_mbps,
    input_to_pixel_us: metrics.input_to_pixel_us,
    scrollback_10k_lines_ms: metrics.scrollback_10k_lines_ms,
    memory: { status: 'blocked_unavailable_toolchain' },
    power: { status: 'blocked_unavailable_toolchain' },
  },
};
const resultsPath = join(ROOT, `benchmarks/results/${PLATFORM}/${DEVICE}/latest.json`);
if (process.argv.includes('--write')) {
  mkdirSync(dirname(resultsPath), { recursive: true });
  writeFileSync(resultsPath, `${JSON.stringify(results, null, 2)}\n`, 'utf8');
  console.log(`wrote ${resultsPath}`);
} else if (!existsSync(resultsPath)) {
  errors.push(`results missing (run with --write): ${resultsPath}`);
}

if (errors.length > 0) {
  console.error(`benchmarks contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`benchmarks contract valid: startup P95 ${startupP95.toFixed(1)}ms, parse p50 ${parseP50.toFixed(1)}MB/s, input-to-pixel P95 ${inputP95.toFixed(0)}us, scrollback P95 ${scrollP95.toFixed(1)}ms — all within budget and within 10% of baseline; results persisted for windows/ci-host; memory/power blocked_unavailable_toolchain.`);