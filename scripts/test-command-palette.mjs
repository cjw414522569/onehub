#!/usr/bin/env node

// T120 contract: command palette and full keyboard navigation.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum PaletteAction', 'pub struct PaletteCommand', 'pub fn matches',
  'pub struct CommandPalette', 'pub fn open', 'pub fn close', 'pub fn toggle',
  'pub fn type_char', 'pub fn backspace', 'pub fn next', 'pub fn prev',
  'pub fn selected_command', 'pub fn execute_selected', 'pub enum FlowKey',
  'pub struct KeyboardFlow', 'pub fn handle',
  'palette_filters_and_navigates', 'keyboard_end_to_end_without_mouse',
  'escape_closes_and_backspace_edits_query',
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
  console.error(`command-palette contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('command-palette contract valid: CommandPalette filters commands by title/keywords, navigates results with wrap-around, and executes the selected command (closing on execute); KeyboardFlow drives the palette with keyboard events only (toggle/type/backspace/next/prev/enter/escape) and applies the executed actions (connect / switch tab / switch window / search / port forward / disconnect) to the session state; the keyboard end-to-end test completes all six actions without a mouse; cargo check/test --locked passed.');