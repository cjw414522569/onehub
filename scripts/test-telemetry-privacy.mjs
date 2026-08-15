#!/usr/bin/env node

// T147 contract: default-off, explicit-consent privacy telemetry.
// Runs the unit suite, the privacy canary (network-capture scan for
// terminal/command/identity/host/canary data), and the schema allowlist
// validation against the public telemetry-schema.json.

import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CANARY = 'PRIVACY_CANARY_3d19f7aa8c';
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Build + unit tests (default-off consent, allowlist, forbidden-field
//    rejection, public schema cleanliness).
const check = run('cargo', ['check', '-p', 'telemetry', '--locked']);
if (check.status !== 0) errors.push(`cargo check -p telemetry failed:\n${check.stdout}\n${check.stderr}`);
const test = run('cargo', ['test', '-p', 'telemetry', '--locked']);
if (test.status !== 0) errors.push(`cargo test -p telemetry failed:\n${test.stdout}\n${test.stderr}`);

// 2. Network-capture scan: run the privacy canary and scan ALL outbound
//    output for forbidden data classes and the canary value.
const canary = run('cargo', ['run', '-p', 'telemetry', '--example', 'privacy-canary']);
if (canary.status !== 0) errors.push(`privacy-canary example failed:\n${canary.stdout}\n${canary.stderr}`);
const combined = `${canary.stdout}\n${canary.stderr}`;
for (const marker of [CANARY, 'ls -la', 'rm -rf /', 'secret.txt', 'c4145', 'db.internal', '10.0.0.5']) {
  if (combined.includes(marker)) errors.push(`forbidden telemetry data leaked: ${marker}`);
}
if (!canary.stdout.includes('capture:consent=off outbound=0')) errors.push('default-off capture must be empty');
if (!canary.stdout.includes('capture:rejected_forbidden=9/9')) errors.push('forbidden fields must all be rejected');
if (!canary.stdout.includes('telemetry:event=app_start platform=windows')) errors.push('allowlisted app_start event missing');
if (!canary.stdout.includes('telemetry:event=feature_used feature=port_forward')) errors.push('allowlisted feature_used event missing');

// 3. Schema allowlist: the public dictionary is off-by-default and never
//    touches terminal / command / identity / host data.
const schema = JSON.parse(readFileSync(join(ROOT, 'crates/telemetry/telemetry-schema.json'), 'utf8'));
if (schema.schema_version !== 1) errors.push('telemetry schema version != 1');
if (schema.consent?.default !== 'off') errors.push('telemetry must be default-off');
if (!Array.isArray(schema.events) || schema.events.length === 0) errors.push('schema events missing');
const forbidden = ['terminal', 'command', 'identity', 'host', 'user', 'key', 'token', 'secret', 'password', 'payload'];
for (const event of schema.events) {
  for (const field of event.fields ?? []) {
    const lower = String(field).toLowerCase();
    if (forbidden.some((token) => lower.includes(token))) {
      errors.push(`schema field '${field}' in ${event.name} touches forbidden data`);
    }
  }
}
const never = (schema.never_collected ?? []).join(' ');
for (const required of ['terminal', 'command', 'identity', 'host', 'credentials', 'keys', 'tokens']) {
  if (!never.includes(required)) errors.push(`never_collected must include: ${required}`);
}

if (errors.length > 0) {
  console.error(`telemetry-privacy contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('telemetry-privacy contract valid: telemetry is default-off and requires explicit consent; the public collection dictionary allowlists every event/field and never contains terminal/command/identity/host data; the network-capture canary scan found zero leaks (off-mode capture empty, 9/9 forbidden fields rejected).');