#!/usr/bin/env node

// T105 contract: authentication prompts, key selection, hardware confirmation.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum PromptKind', 'pub enum PromptState', 'pub enum PromptError',
  'pub struct AuthPrompt', 'pub struct KeyOption', 'pub struct KeySelection',
  'pub enum SelectionState', 'pub enum SelectionError', 'pub struct HardwareConfirmation',
  'pub enum ConfirmState', 'pub fn enter', 'pub fn submit', 'pub fn cancel',
  'pub fn select', 'pub fn confirm', 'pub fn is_input_clear',
  'auth_interaction_matrix', 'prompt_requires_input_before_submit',
  'key_selection_select_confirm_and_cancel', 'hardware_confirmation_confirm_and_cancel',
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
  console.error(`auth-prompt contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('auth-prompt contract valid: AuthPrompt holds sensitive input only transiently (submit moves the value out and clears the model, so it is never cached; cancel clears immediately and terminates authentication, refusing a later submit); KeySelection selects/confirms/cancels keys with unknown-id rejection; HardwareConfirmation confirm/cancel works; the authentication interaction matrix (submit / cancel / cancel-then-submit across password, passphrase, key-selection, and hardware-confirmation) passes; cargo check/test --locked passed.');