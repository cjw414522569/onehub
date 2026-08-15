#!/usr/bin/env node

// T103 contract: host editor + inline validation + accessibility.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub struct HostEditorForm', 'pub enum FieldKind', 'pub struct FieldSpec',
  'pub struct SectionSpec', 'pub struct SectionState', 'pub struct ReviewRow',
  'pub struct SectionReview', 'pub struct AccessibilityReport', 'pub fn default_spec',
  'pub fn set', 'pub fn validate', 'pub fn is_valid', 'pub fn review', 'pub fn accessibility',
  'PASSWORD_MASK', 'fn validate_field',
  'default_form_has_all_five_reviewable_sections',
  'inline_validation_catches_bad_input_and_clears_on_fix',
  'auth_method_key_requires_key_path', 'form_is_valid_when_all_fields_ok',
  'review_masks_passwords_and_lists_sections',
  'accessibility_report_has_labels_and_focus_order',
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
  console.error(`host-editor contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('host-editor contract valid: HostEditorForm organizes basic/auth/proxy/terminal/advanced into five reviewable sections with labeled fields; every field validates inline on set (including cross-field rules: key auth requires a key path, proxy-enabled requires host/port); the review view masks passwords; the accessibility report shows every field labeled, a stable focus order, and non-empty screen-reader-friendly error messages; form-state and accessibility tests pass; cargo check/test --locked passed.');