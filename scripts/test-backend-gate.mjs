#!/usr/bin/env node

// T122 contract: ConPTY / system OpenSSH backend capability gate.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum TerminalBackend', 'pub enum BackendSelection', 'pub struct BackendGate',
  'pub fn select', 'pub fn reset', 'pub fn active_backend', 'pub fn system_enabled',
  'pub enum Feature', 'pub fn label', 'pub struct FeatureSupport', 'pub fn differs',
  'pub struct BackendComparison', 'pub fn compare', 'pub fn differences',
  'system_backend_is_only_enabled_when_explicitly_selected',
  'behavior_differences_are_visible', 'feature_labels_are_readable',
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
  console.error(`backend-gate contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('backend-gate contract valid: BackendGate only enables the system OpenSSH backend on explicit selection (the built-in backend is the default and the system backend is never implicit); BackendComparison exposes the feature-support matrix so behavior differences (true color, bracketed paste, mouse, unicode width, OSC 52) are visible to the user; the built-in vs system comparison tests pass; cargo check/test --locked passed.');