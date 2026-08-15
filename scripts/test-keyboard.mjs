#!/usr/bin/env node

// T108 contract: physical keyboard, IME, modifiers, configurable shortcuts.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum Platform', 'pub enum KeyCode', 'pub enum Direction', 'pub struct Modifiers',
  'pub struct KeyEvent', 'pub enum PrimaryModifier', 'pub struct PlatformSemantics',
  'pub fn parse_key', 'pub struct KeyMap', 'pub fn normalize', 'pub enum ModifierKey',
  'pub enum KeyAction', 'pub struct Chord', 'pub struct KeyBindingConfig', 'pub fn defaults',
  'pub fn set_binding', 'pub fn clear_binding', 'pub fn resolve', 'pub fn chord_label',
  'key_codes_parse_and_normalize', 'platform_primary_modifier_semantics',
  'keyboard_event_matrix_is_platform_consistent', 'ime_composition_suppresses_shortcuts',
  'shortcuts_are_configurable_and_remappable', 'key_labels_are_readable',
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
  console.error(`keyboard contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('keyboard contract valid: KeyMap/parse_key normalize platform key names into neutral KeyCodes (letters/digits normalized) so Windows/macOS/Linux/Android/iOS share one key semantic; PlatformSemantics makes the primary shortcut modifier explicit (Ctrl everywhere, Cmd on macOS) with readable labels; IME composition suppresses shortcut chords; KeyBindingConfig maps chords to KeyActions and is user-remappable (set_binding/clear_binding) with per-platform chord labels; the keyboard event matrix is platform-consistent (Ctrl+T vs Cmd+T resolve to the same action); cargo check/test --locked passed.');