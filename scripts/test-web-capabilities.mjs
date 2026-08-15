#!/usr/bin/env node

// T133 Web capability contract: lint + reproducible snapshot + gating check.

import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

// 1. Lint.
const lint = spawnSync('node', [join(ROOT, 'scripts', 'lint-web-capabilities.mjs'), ROOT], {
  cwd: ROOT, encoding: 'utf8', timeout: 60000,
});
if (lint.status !== 0) {
  errors.push(`web-capabilities lint failed:\n${lint.stdout}\n${lint.stderr}`);
}

// 2. Reproducible snapshot.
const model = JSON.parse(readFileSync(join(ROOT, 'web', 'capabilities.json'), 'utf8'));
const snapshotPath = join(ROOT, 'web', 'capabilities.snapshot.json');
const serialized = `${JSON.stringify(model, null, 2)}\n`;
const tmp = join(ROOT, 'artifacts/tmp/web-capabilities');
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
writeFileSync(join(tmp, 'capabilities.snapshot.json'), serialized, 'utf8');
if (!existsSync(snapshotPath)) {
  writeFileSync(snapshotPath, serialized, 'utf8');
  errors.push('snapshot missing; wrote capabilities.snapshot.json (commit and re-run)');
} else if (readFileSync(snapshotPath, 'utf8') !== serialized) {
  errors.push('snapshot drift: capabilities.snapshot.json differs from regeneration');
}

// 3. UI gating consistency: no capability is both gated and usable.
for (const [name, capability] of Object.entries(model.capabilities ?? {})) {
  if (capability.gated && capability.status === 'available') {
    errors.push(`UI gating inconsistency for ${name}`);
  }
}

if (errors.length > 0) {
  console.error(`web-capabilities contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('web-capabilities contract valid: the model is lint-clean, its snapshot regenerates byte-identically, raw TCP is explicitly unavailable, unavailable capabilities are listed with user-visible notes, and UI gating is consistent.');