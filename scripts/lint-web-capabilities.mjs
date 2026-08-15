#!/usr/bin/env node

// T133 Web capability model lint: raw TCP unavailable, statuses valid,
// and UI gating consistent (a gated capability is never shown as usable).

import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const path = join(ROOT, 'web', 'capabilities.json');
const model = JSON.parse(readFileSync(path, 'utf8'));
const errors = [];

if (model.schema_version !== 1) errors.push('schema_version must be 1');
if (model.raw_tcp?.status !== 'unavailable') {
  errors.push('raw_tcp must be unavailable (browsers cannot raw-TCP connect)');
}
if (!model.raw_tcp?.user_note) errors.push('raw_tcp must carry a user-visible note');

for (const [name, capability] of Object.entries(model.capabilities ?? {})) {
  if (!['available', 'partial', 'unavailable'].includes(capability.status)) {
    errors.push(`${name}: invalid status ${capability.status}`);
  }
  if (typeof capability.gated !== 'boolean') errors.push(`${name}: gated must be a boolean`);
  if (!capability.user_note || capability.user_note.length === 0) {
    errors.push(`${name}: missing user-visible note`);
  }
  // UI gating consistency: a gated capability is shown as unavailable.
  if (capability.gated && capability.status === 'available') {
    errors.push(`${name}: gated capability must not be shown as available`);
  }
}

if (errors.length > 0) {
  console.error(`web-capabilities lint failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('web-capabilities lint valid: raw TCP is unavailable, every capability has a valid status + user-visible note, and UI gating is consistent (gated capabilities are never shown as available).');