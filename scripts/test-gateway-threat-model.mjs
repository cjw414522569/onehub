#!/usr/bin/env node

// T134 gateway threat-model contract: lint + security review gate + snapshot.

import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

const lint = spawnSync('node', [join(ROOT, 'scripts', 'lint-gateway-threat-model.mjs'), ROOT], {
  cwd: ROOT, encoding: 'utf8', timeout: 60000,
});
if (lint.status !== 0) {
  errors.push(`gateway-threat-model lint failed:\n${lint.stdout}\n${lint.stderr}`);
}

const model = JSON.parse(readFileSync(join(ROOT, 'security', 'gateway-threat-model.json'), 'utf8'));
const snapshotPath = join(ROOT, 'security', 'gateway-threat-model.snapshot.json');
const serialized = `${JSON.stringify(model, null, 2)}\n`;
const tmp = join(ROOT, 'artifacts/tmp/gateway-threat-model');
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
writeFileSync(join(tmp, 'gateway-threat-model.snapshot.json'), serialized, 'utf8');
if (!existsSync(snapshotPath)) {
  writeFileSync(snapshotPath, serialized, 'utf8');
  errors.push('snapshot missing; wrote gateway-threat-model.snapshot.json (commit and re-run)');
} else if (readFileSync(snapshotPath, 'utf8') !== serialized) {
  errors.push('snapshot drift: gateway-threat-model.snapshot.json differs from regeneration');
}

if (errors.length > 0) {
  console.error(`gateway-threat-model contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gateway-threat-model contract valid: the security review gate passes (SSRF, internal access, credentials, audit, rate limiting, and abuse all have controls), the tenant boundary and deployment topology are explicit, and the snapshot regenerates byte-identically.');