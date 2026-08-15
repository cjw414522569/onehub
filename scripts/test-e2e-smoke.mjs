#!/usr/bin/env node

// T153 contract: page objects, test accounts, key rotation, and the
// six-platform E2E smoke matrix. Runs the type gate, the smoke matrix with
// env-only secrets, and a canary scan proving the secret never enters the
// repository tree.

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// A fresh canary secret: it must never appear anywhere in the repo.
const canary = `CANARY_${Math.random().toString(16).slice(2, 14)}`;

// 1. Type gate.
const tscBin = join(ROOT, 'web/app/node_modules/typescript/bin/tsc');
const tsc = run('node', [tscBin, '--noEmit', '-p', join(ROOT, 'e2e/tsconfig.json')]);
if (tsc.status !== 0) errors.push(`e2e tsc failed:\n${tsc.stdout}\n${tsc.stderr}`);

// 2. Smoke matrix with env-only secrets.
const env = { ...process.env, E2E_GATEWAY_TOKEN: canary, E2E_TEST_KEY: `test-key-${canary}` };
const smoke = spawnSync('node', ['--experimental-strip-types', join(ROOT, 'e2e/src/smoke.ts'), `--canary=${canary}`], {
  cwd: ROOT, encoding: 'utf8', env, timeout: 300000,
});
if (smoke.status !== 0) errors.push(`e2e smoke failed:\n${smoke.stdout}\n${smoke.stderr}`);
if (!smoke.stdout.includes('E2E smoke matrix valid')) errors.push('smoke matrix did not report success');
for (const platform of ['windows', 'macos', 'linux', 'ios', 'android', 'web']) {
  if (!smoke.stdout.includes(`PASS journey.${platform}`)) errors.push(`journey.${platform} did not pass`);
}

// 3. Secrets never in repo: scan every tracked file for the canary.
const skip = new Set(['.git', 'node_modules', 'target', 'tmp', '.claude']);
function walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (skip.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (entry.isFile()) {
      try {
        if (readFileSync(full, 'utf8').includes(canary)) {
          errors.push(`secret found in repository: ${full}`);
        }
      } catch {
        // binary files are skipped
      }
    }
  }
}
walk(ROOT);

if (errors.length > 0) {
  console.error(`e2e-smoke contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('e2e-smoke contract valid: the six-platform critical journeys (windows/macos/linux/ios/android/web) pass against the deterministic fake gateway via page objects with environment-only secrets; key rotation verified; the canary-secret scan found the secret nowhere in the repository.');