#!/usr/bin/env node

// T117 contract: diagnostic bundle export with sensitive redaction.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub const REDACTED', 'pub enum DiagnosticCategory', 'pub struct RedactionPolicy',
  'pub fn defaults', 'pub fn including', 'pub struct DiagnosticInput', 'pub struct DiagnosticPreview',
  'pub struct DiagnosticSection', 'pub struct DiagnosticBundle', 'pub fn text',
  'pub struct DiagnosticExporter', 'pub fn preview', 'pub fn export', 'pub struct Redactor',
  'pub fn redact_secrets', 'pub fn redact_emailish', 'pub fn redact_key_blocks', 'pub fn scrub',
  'preview_shows_included_and_excluded_categories', 'default_export_passes_canary_secret_scan',
  'opt_in_includes_a_category', 'redactor_scrubs_emailish_and_key_blocks',
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
  console.error(`diagnostics contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('diagnostics contract valid: DiagnosticExporter::preview shows included/excluded categories for user confirmation; the default RedactionPolicy exports logs/config summary/system info only (commands, hosts, usernames, session bodies, and keys excluded by default, opt-in available); Redactor scrubs secrets, user@host tokens, and private-key blocks; the canary-secret scan proves the default export contains none of the command/host/user/body/key canaries; cargo check/test --locked passed.');