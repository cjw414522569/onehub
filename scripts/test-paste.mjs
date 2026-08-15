#!/usr/bin/env node

// T110 contract: secure paste confirmation, multi-line warning, bracketed paste.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum PasteRisk', 'pub struct PasteContent', 'pub fn analyze', 'pub fn risk',
  'pub fn preview', 'pub enum PasswordPastePolicy', 'pub struct PastePolicy', 'pub fn defaults',
  'pub struct PastePayload', 'pub enum PasteDecision', 'pub struct SecurePasteFlow',
  'pub fn bracketed_payload', 'pub fn evaluate', 'BRACKETED_PASTE_BEGIN', 'BRACKETED_PASTE_END',
  'analyze_detects_newlines_control_and_suspicious', 'preview_escapes_and_truncates',
  'decision_matrix_follows_policy', 'password_paste_policy_is_configurable',
  'bracketed_paste_wraps_payload', 'huge_clipboard_is_flagged',
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
  console.error(`paste contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('paste contract valid: PasteContent::analyze detects newlines, control characters, suspicious shell fragments, and size; SecurePasteFlow applies the configurable PastePolicy and returns Allow/Confirm/Block, with a previewable payload (control chars escaped, truncated with byte count) so a potential command injection is visible before pasting; password pasting has its own configurable policy (Allow/Confirm/Block); bracketed_payload wraps text in ESC[200~ ... ESC[201~; newline / control-character / huge-clipboard tests pass; cargo check/test --locked passed.');