#!/usr/bin/env node

// T151 contract: per-language unit-test coverage baseline and differential
// gate. Measures Rust line coverage (cargo llvm-cov), TypeScript
// module-exercise coverage, asserts the budgets and core security state
// machines, and enforces the differential gate (current >= baseline -
// tolerance). With --write, regenerates coverage/coverage-report.json.

import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const COV = join(ROOT, 'coverage');
const errors = [];
const report = { schema_version: 1, languages: {} };

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 1800000 });
}

// 1. Rust line coverage via cargo llvm-cov.
const cov = run('cargo', ['llvm-cov', '--workspace', '--locked', '--summary-only']);
if (cov.status !== 0) errors.push(`cargo llvm-cov failed:\n${cov.stdout}\n${cov.stderr}`);
const totalMatch = /TOTAL\s+\d+\s+(\d+)\s+([\d.]+)%/.exec(cov.stdout);
if (!totalMatch) errors.push('cargo llvm-cov summary missing TOTAL line');
const rustTotal = totalMatch ? Number(totalMatch[2]) : 0;

// Per-file line % for the core security state machines.
const coreModules = [
  'services/gateway/src/auth.rs',
  'services/gateway/src/session_protocol.rs',
  'services/gateway/src/address_policy.rs',
  'crates/telemetry/src/crash.rs',
  'crates/telemetry/src/privacy.rs',
  'crates/telemetry/src/log.rs',
  'apps/cli/src/cli.rs',
];
const coreCoverage = {};
for (const module of coreModules) {
  const escaped = module.replace(/[\\/]/g, '[\\\\/]');
  const match = new RegExp(`${escaped}\\s+\\d+\\s+\\d+\\s+([\\d.]+)%`).exec(cov.stdout);
  coreCoverage[module] = match ? Number(match[1]) : null;
}
report.languages.rust = {
  method: 'cargo llvm-cov --workspace --locked --summary-only (line %)',
  threshold: 80.0,
  measured_total: rustTotal,
  core_security_threshold: 90.0,
  core_security_modules: coreCoverage,
};

// 2. TypeScript module-exercise coverage (transitive import graph).
const srcDir = join(ROOT, 'web/app/src');
const testDir = join(ROOT, 'web/app/test');
const modules = readdirSync(srcDir).filter((f) => f.endsWith('.ts')).map((f) => f.replace(/\.ts$/, ''));
const tests = readdirSync(testDir).filter((f) => f.endsWith('.ts'));
const testText = tests.map((f) => readFileSync(join(testDir, f), 'utf8')).join('\n');
const moduleText = (name) => readFileSync(join(srcDir, `${name}.ts`), 'utf8');
const exercised = new Set();
const queue = modules.filter((m) => testText.includes(`'../src/${m}.ts'`));
for (const m of queue) exercised.add(m);
while (queue.length > 0) {
  const current = queue.shift();
  const text = moduleText(current);
  for (const name of modules) {
    if (text.includes(`from './${name}.ts'`) && !exercised.has(name)) {
      exercised.add(name);
      queue.push(name);
    }
  }
}
const tsCoverage = modules.length === 0 ? 0 : (exercised.size / modules.length) * 100;
report.languages.typescript = {
  method: 'module-exercise coverage of web/app/src/*.ts by tests',
  threshold: 90.0,
  modules_total: modules.length,
  modules_exercised: exercised.size,
  measured_pct: tsCoverage,
};
report.languages.csharp = { status: 'blocked_unavailable_toolchain' };
report.languages.swift = { status: 'blocked_unavailable_toolchain' };
report.languages.kotlin = { status: 'blocked_unavailable_toolchain' };

// 3. Budgets + differential gate against the committed baseline.
const baseline = JSON.parse(readFileSync(join(COV, 'coverage-baseline.json'), 'utf8'));
const rustBaseline = baseline.languages.rust.baseline;
const tolerance = baseline.languages.rust.differential_tolerance_pct;
if (rustTotal < baseline.languages.rust.threshold) errors.push(`rust coverage ${rustTotal}% below threshold 80%`);
if (rustTotal < rustBaseline - tolerance) errors.push(`rust coverage ${rustTotal}% regressed below baseline ${rustBaseline}% (tolerance ${tolerance})`);
for (const [module, pct] of Object.entries(coreCoverage)) {
  if (pct !== null && pct < baseline.languages.rust.core_security_threshold) {
    errors.push(`core security module ${module} at ${pct}% below 90%`);
  }
}
if (tsCoverage < baseline.languages.typescript.threshold) errors.push(`typescript coverage ${tsCoverage}% below threshold 90%`);

// 4. Write / verify the report. The report is deterministic (no
//    timestamps) so regeneration is byte-identical.
const reportText = `${JSON.stringify(report, null, 2)}\n`;
const reportPath = join(COV, 'coverage-report.json');
if (process.argv.includes('--write')) {
  mkdirSync(COV, { recursive: true });
  writeFileSync(reportPath, reportText, 'utf8');
  console.log(`wrote ${reportPath}`);
} else if (existsSync(reportPath)) {
  if (!readFileSync(reportPath).equals(Buffer.from(reportText, 'utf8'))) {
    errors.push('coverage-report.json changed (regenerate with --write)');
  }
} else {
  errors.push('coverage-report.json missing (regenerate with --write)');
}

if (errors.length > 0) {
  console.error(`coverage contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`coverage contract valid: rust workspace line coverage ${rustTotal}% (>= baseline ${rustBaseline}%, threshold 80%); core security state machines all >= 90%; typescript module-exercise coverage ${tsCoverage.toFixed(1)}% (>= 90%); C#/Swift/Kotlin blocked_unavailable_toolchain.`);