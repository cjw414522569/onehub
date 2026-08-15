#!/usr/bin/env node

// T140 contract: PWA offline shell, update, and data-clearing policy —
// type gate, service-worker upgrade + cache audit E2E, and snapshot check.

import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const APP = join(ROOT, 'web/app');
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: opts.cwd ?? APP, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Type gate: strict tsc --noEmit.
const tscBin = join(APP, 'node_modules/typescript/bin/tsc');
if (!existsSync(tscBin)) {
  const install = run('npm.cmd', ['ci'], { timeout: 600000 });
  if (install.status !== 0) errors.push(`npm ci failed:\n${install.stdout}\n${install.stderr}`);
}
const tsc = run('node', [tscBin, '--noEmit', '-p', join(APP, 'tsconfig.json')]);
if (tsc.status !== 0) errors.push(`tsc --noEmit failed:\n${tsc.stdout}\n${tsc.stderr}`);

// 2. Service-worker upgrade + cache audit E2E (headless, deterministic).
const e2e = run('node', ['--experimental-strip-types', join(APP, 'test/pwa-offline.ts')]);
if (e2e.status !== 0) errors.push(`PWA offline E2E failed:\n${e2e.stdout}\n${e2e.stderr}`);

// 3. Cache-audit snapshot: regenerate and compare byte-for-byte.
const temp = mkdtempSync(join(tmpdir(), 'pwa-offline-snapshot-'));
const gen = run('node', ['--experimental-strip-types', join(APP, 'test/pwa-offline.ts'), '--write', temp]);
if (gen.status !== 0) {
  errors.push(`snapshot generation failed:\n${gen.stdout}\n${gen.stderr}`);
} else {
  const committed = join(APP, 'pwa/cache-audit.snapshot.json');
  const generated = join(temp, 'cache-audit.snapshot.json');
  if (!existsSync(committed)) {
    errors.push('committed pwa/cache-audit.snapshot.json missing (regenerate with --write)');
  } else if (!existsSync(generated)) {
    errors.push('generated cache-audit.snapshot.json missing');
  } else if (!readFileSync(committed).equals(readFileSync(generated))) {
    errors.push('cache-audit.snapshot.json is not byte-identical to the committed golden');
  }
}
rmSync(temp, { recursive: true, force: true });

if (errors.length > 0) {
  console.error(`pwa-offline contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('pwa-offline contract valid: strict TypeScript compiles; the service-worker upgrade flow purges stale caches, the cache policy/audit holds no session secrets, clear-session-data keeps the app shell, and offline never claims connectivity; the cache-audit snapshot regenerates byte-identical.');