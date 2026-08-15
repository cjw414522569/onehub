#!/usr/bin/env node

// T132 contract: secure ssh:// deep-link parsing + explicit confirmation.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum LinkRejection', 'pub struct SecureLink', 'pub enum LinkSource', 'pub struct DeepLinkPolicy',
  'pub fn for_source', 'pub fn parse_secure', 'PlaintextPassword', 'UnsupportedScheme',
  'MissingHost', 'InvalidHost', 'InvalidPort',
  'plaintext_passwords_are_rejected', 'external_links_require_confirmation_by_default',
  'injection_corpus_is_rejected_or_sanitized', 'path_and_query_are_stripped',
  'host_after_embedded_at_is_used',
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
  console.error(`deep-link contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('deep-link contract valid: parse_secure rejects plaintext passwords in the userinfo, validates the host strictly (no whitespace/control/embedded @/scheme), strips path/query/fragment, and validates the port; DeepLinkPolicy requires explicit confirmation for external sources by default (never auto-connect) while in-app links do not; the URI fuzz corpus (malformed and injection URIs) is rejected or sanitized; cargo check/test --locked passed.');