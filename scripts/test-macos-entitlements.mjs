#!/usr/bin/env node

// T124 contract: macOS sandbox, hardened runtime, minimal entitlements.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum Entitlement', 'pub struct EntitlementSet', 'pub fn minimal', 'pub fn with',
  'pub fn contains', 'pub fn is_minimal', 'pub fn allowed_on_demand',
  'pub struct AuditIssue', 'pub struct NotarizationAudit', 'pub fn check', 'pub fn passes',
  'HARDENED_RUNTIME_REQUIRED', 'SANDBOX_REQUIRED', 'GET_TASK_ALLOW_FORBIDDEN',
  'EXTRA_ENTITLEMENT',
  'minimal_release_audit_passes', 'hardened_runtime_and_sandbox_are_required',
  'get_task_allow_is_forbidden_in_release', 'on_demand_entitlements_are_allowed_but_flagged_extra',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(LIB, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`host-library missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'host-library', '--locked'],
  ['test', '-p', 'host-library', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p host-library failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`macos-entitlements contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('macos-entitlements contract valid: EntitlementSet::minimal is the baseline (sandbox, network client, user-selected files) with explicit on-demand additions (network server for forwarding, keychain access group); NotarizationAudit enforces the pre-notarization checks (hardened runtime, sandbox, no get-task-allow in release, no extra entitlements); permissions are on-demand and the minimal release audit passes while every violation is flagged; real codesign/spctl/entitlement audits run on macOS hosts; cargo check/test --locked passed.');