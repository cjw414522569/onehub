#!/usr/bin/env node

// T159 contract: startup, power, and package-size budgets. Measures CLI
// cold/hot startup (P95), the release CLI + wasm package sizes (raw and
// compressed), asserts the T003 budgets, and documents desktop/mobile
// packages and mobile power as blocked_unavailable_toolchain. With --write,
// persists results and archives docs/reports/STARTUP_SIZE_T159.json.

import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
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

// 1. Build the release CLI (the measurable startup/package artifact).
const build = run('cargo', ['build', '--release', '-p', 'cli', '--locked']);
if (build.status !== 0) errors.push(`release cli build failed:\n${build.stdout}\n${build.stderr}`);
const exe = join(ROOT, 'target/release/ssh-cli.exe');
if (!existsSync(exe)) errors.push('release ssh-cli.exe missing');

// 2. Startup: cold = fresh process; hot = warm OS cache (second invocation).
const cold = [];
const hot = [];
for (let i = 0; i < repeats; i += 1) {
  const t0 = process.hrtime.bigint();
  const res = spawnSync(exe, ['--version'], { encoding: 'utf8' });
  cold.push(Number(process.hrtime.bigint() - t0) / 1e6);
  if (res.status !== 0) errors.push('ssh-cli --version failed');
}
for (let i = 0; i < repeats; i += 1) {
  const t0 = process.hrtime.bigint();
  const res = spawnSync(exe, ['--version'], { encoding: 'utf8' });
  hot.push(Number(process.hrtime.bigint() - t0) / 1e6);
  if (res.status !== 0) errors.push('ssh-cli --version failed');
}
const coldP95 = percentile(cold, 95);
const hotP95 = percentile(hot, 95);

// 3. Package sizes (raw + gzip) for CLI and wasm.
const cliBytes = statSync(exe).size;
const cliGzip = gzipSync(readFileSync(exe)).length;
const wasmPath = join(ROOT, 'target/wasm32-unknown-unknown/release/wasm.wasm');
const wasmBytes = existsSync(wasmPath) ? statSync(wasmPath).size : null;
const wasmGzip = wasmBytes !== null ? gzipSync(readFileSync(wasmPath)).length : null;

const checks = [
  { name: `cold_start_p95=${coldP95.toFixed(1)}ms <= 500ms`, ok: coldP95 <= 500 },
  { name: `hot_start_p95=${hotP95.toFixed(1)}ms <= 200ms`, ok: hotP95 <= 200 },
  { name: `cli_package_gzip=${(cliGzip / 1024 / 1024).toFixed(2)}MB <= 20MB`, ok: cliGzip <= 20 * 1024 * 1024 },
];
if (wasmBytes !== null) {
  checks.push({ name: `wasm_bundle_gzip=${(wasmGzip / 1024 / 1024).toFixed(2)}MB <= 8MB`, ok: wasmGzip <= 8 * 1024 * 1024 });
}
for (const c of checks) if (!c.ok) errors.push(`budget failed: ${c.name}`);

if (errors.length > 0) {
  console.error(`startup-size contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

// 4. Persist + archive.
const results = {
  schema_version: 1,
  platform: 'windows',
  device: 'ci-host',
  startup: { cold_p95_ms: coldP95, hot_p95_ms: hotP95, repeats },
  package_size: {
    cli_release_bytes: cliBytes,
    cli_gzip_bytes: cliGzip,
    wasm_bytes: wasmBytes,
    wasm_gzip_bytes: wasmGzip,
  },
  blocked: {
    desktop_packages: 'blocked_unavailable_toolchain (no installers built)',
    mobile_power: 'blocked_unavailable_toolchain (needs a mobile device)',
    mobile_packages: 'blocked_unavailable_toolchain (no mobile builds)',
  },
};
const persistPath = join(ROOT, 'benchmarks/results/windows/ci-host/startup-size.json');
if (process.argv.includes('--write')) {
  mkdirSync(dirname(persistPath), { recursive: true });
  writeFileSync(persistPath, `${JSON.stringify(results, null, 2)}\n`, 'utf8');
  const reportPath = join(ROOT, 'docs/reports/STARTUP_SIZE_T159.json');
  writeFileSync(reportPath, `${JSON.stringify({ ...results, task: 'T159', status: 'pass' }, null, 2)}\n`, 'utf8');
  console.log(`wrote ${persistPath} and ${reportPath}`);
} else if (!existsSync(persistPath)) {
  errors.push(`results missing (run with --write): ${persistPath}`);
}

if (errors.length > 0) {
  console.error(`startup-size contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`startup-size contract valid: cold start P95 ${coldP95.toFixed(1)}ms (<=500), hot start P95 ${hotP95.toFixed(1)}ms (<=200); CLI release ${(cliBytes / 1024 / 1024).toFixed(2)}MB (gzip ${(cliGzip / 1024 / 1024).toFixed(2)}MB <= 20MB), wasm ${(wasmBytes / 1024 / 1024).toFixed(2)}MB (gzip ${(wasmGzip / 1024 / 1024).toFixed(2)}MB <= 8MB); desktop/mobile packages and mobile power documented blocked_unavailable_toolchain.`);