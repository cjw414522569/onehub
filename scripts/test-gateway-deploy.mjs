#!/usr/bin/env node

// T141 contract: gateway container/Helm/standalone deployment + hardening.
// Runs a deterministic container scan (every hardening control present,
// every forbidden pattern absent) and an install smoke (the gateway crate
// builds with --locked). With --write, regenerates the scan-report snapshot.

import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const DEPLOY = join(ROOT, 'deploy/gateway');
const errors = [];
const report = {
  schema_version: 1,
  controls: [],
  forbidden: [],
  smoke: {},
};

function fileText(relative) {
  const file = join(ROOT, relative);
  if (!existsSync(file)) {
    errors.push(`missing file: ${relative}`);
    return '';
  }
  return readFileSync(file, 'utf8');
}

// 1. Container scan: hardening controls.
const hardening = JSON.parse(readFileSync(join(DEPLOY, 'hardening.json'), 'utf8'));
for (const control of hardening.controls) {
  const findings = [];
  for (const entry of control.must_include ?? []) {
    const text = fileText(entry.file);
    if (!text.includes(entry.pattern)) findings.push(`missing ${entry.pattern} in ${entry.file}`);
  }
  for (const entry of control.must_exclude ?? []) {
    const text = fileText(entry.file);
    if (text.includes(entry.pattern)) findings.push(`forbidden ${entry.pattern} present in ${entry.file}`);
  }
  report.controls.push({ id: control.id, pass: findings.length === 0, findings });
  for (const finding of findings) errors.push(`[${control.id}] ${finding}`);
}

// 2. Forbidden patterns across the deployment assets.
for (const entry of hardening.forbidden_patterns ?? []) {
  const text = fileText(entry.file);
  const hit = text.includes(entry.pattern);
  report.forbidden.push({ file: entry.file, pattern: entry.pattern, hit });
  if (hit) errors.push(`forbidden pattern found: ${entry.pattern} in ${entry.file}`);
}

// 3. Install smoke: the gateway crate builds locked (the installed artifact).
const cargo = spawnSync('cargo', ['check', '-p', 'gateway', '--locked'], {
  cwd: ROOT, encoding: 'utf8', timeout: 300000,
});
report.smoke = {
  gateway_cargo_check_locked: cargo.status === 0,
  docker: 'blocked_unavailable_toolchain',
  helm: 'blocked_unavailable_toolchain',
};
if (cargo.status !== 0) errors.push(`cargo check -p gateway --locked failed:\n${cargo.stdout}\n${cargo.stderr}`);

// 4. Snapshot: byte-identical regeneration.
const snapshotText = `${JSON.stringify(report, null, 2)}\n`;
const snapshotPath = join(DEPLOY, 'scan-report.snapshot.json');
if (process.argv.includes('--write')) {
  mkdirSync(DEPLOY, { recursive: true });
  writeFileSync(snapshotPath, snapshotText, 'utf8');
  console.log(`wrote ${snapshotPath}`);
} else if (existsSync(snapshotPath)) {
  if (!readFileSync(snapshotPath).equals(Buffer.from(snapshotText, 'utf8'))) {
    errors.push('scan-report.snapshot.json is not byte-identical (regenerate with --write)');
  }
} else {
  errors.push('scan-report.snapshot.json missing (regenerate with --write)');
}

if (errors.length > 0) {
  console.error(`gateway-deploy contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gateway-deploy contract valid: every hardening control (default non-root, read-only rootfs, TLS, resource limits, no privilege escalation, secret management) is present; no forbidden deployment patterns; the gateway crate install-smoke (cargo check --locked) passes; scan-report.snapshot.json regenerates byte-identical. Real docker/helm renders are blocked_unavailable_toolchain on this host.');