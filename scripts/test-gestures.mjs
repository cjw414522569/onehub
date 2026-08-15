#!/usr/bin/env node

// T109 contract: mobile terminal extended keyboard and gestures.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub struct TouchPoint', 'pub enum ExtendedKey', 'pub enum Gesture', 'pub enum InputMode',
  'pub struct KeyChord', 'pub struct ExtendedKeyboard', 'pub struct GestureRecognizer',
  'pub fn press', 'pub fn release', 'pub fn on_touch_down', 'pub fn on_touch_move',
  'pub fn on_touch_up', 'pub fn set_mode', 'pub fn mode', 'LONG_PRESS_MS', 'SCROLL_THRESHOLD',
  'tap_long_press_and_scroll_disambiguate', 'selection_mode_drag_selects_not_scrolls',
  'extended_keys_do_not_conflict_with_scroll_or_selection', 'esc_and_alt_keys_emit_chords',
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
  console.error(`gestures contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gestures contract valid: GestureRecognizer deterministically disambiguates tap / long-press / scroll (long press starts a selection, a drag past the threshold scrolls in normal mode and extends the selection in selection mode, a tap ends the selection); ExtendedKeyboard emits Ctrl/Alt/Esc/Tab/arrow chords independently of the touch canvas, so extended keys never conflict with scroll or selection (verified mid-scroll and mid-selection); touch gesture integration tests pass; cargo check/test --locked passed.');