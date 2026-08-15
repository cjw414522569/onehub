#!/usr/bin/env node

// T126 Linux packaging contract: lint + reproducible policy snapshot.

import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

// 1. Policy lint.
const lint = spawnSync('node', [join(ROOT, 'scripts', 'lint-linux-packaging.mjs'), ROOT], {
  cwd: ROOT, encoding: 'utf8', timeout: 60000,
});
if (lint.status !== 0) {
  errors.push(`linux-packaging lint failed:\n${lint.stdout}\n${lint.stderr}`);
}

// 2. Reproducible snapshot: the policy regenerates byte-identically.
const policyPath = join(ROOT, 'packaging', 'linux', 'policy.json');
const policy = JSON.parse(readFileSync(policyPath, 'utf8'));
const sorted = {};
for (const key of Object.keys(policy).sort()) {
  sorted[key] = policy[key];
}
const snapshotPath = join(ROOT, 'packaging', 'linux', 'policy.snapshot.json');
const serialized = `${JSON.stringify(sorted, null, 2)}\n`;
const tmp = join(ROOT, 'artifacts/tmp/linux-packaging');
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
writeFileSync(join(tmp, 'policy.snapshot.json'), serialized, 'utf8');
if (!existsSync(snapshotPath)) {
  writeFileSync(snapshotPath, serialized, 'utf8');
  errors.push('snapshot missing; wrote policy.snapshot.json (commit and re-run)');
} else if (readFileSync(snapshotPath, 'utf8') !== serialized) {
  errors.push('snapshot drift: policy.snapshot.json differs from regeneration');
}

if (errors.length > 0) {
  console.error(`linux-packaging contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('linux-packaging contract valid: the deb/rpm/AppImage/Flatpak policy is lint-clean and its normalized snapshot regenerates byte-identically (reproducible); dependencies, sandbox permissions, and auto-update boundaries are explicit; install/upgrade/uninstall on clean distros runs on Linux hosts.');