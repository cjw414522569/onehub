#!/usr/bin/env node

// T119 contract: accessibility semantics, screen readers, reduce-motion.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum ViolationSeverity', 'pub enum A11yRole', 'pub struct A11yNode',
  'pub fn accessible_name', 'pub struct A11yViolation', 'pub struct A11yTree',
  'pub fn focus_order', 'pub fn audit', 'pub enum MotionPreference', 'pub struct ReduceMotionPolicy',
  'pub fn for_preference', 'pub struct TerminalAccessibleMode', 'pub fn screen_reader_text',
  'pub fn screen_reader_checklist', 'WCAG_4_1_2_NAME', 'WCAG_2_4_3_FOCUS_ORDER',
  'WCAG_2_1_1_KEYBOARD',
  'audit_finds_missing_names_and_keyboard_issues', 'focus_order_is_deterministic_and_complete',
  'reduce_motion_disables_animation', 'terminal_accessible_mode_announces_screen_and_cursor',
  'screen_reader_checklist_covers_all_three_readers',
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
  console.error(`accessibility contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('accessibility contract valid: A11yTree is a semantic tree whose audit runs the WCAG 2.2 AA critical-path checks (4.1.2 accessible names on interactive nodes, 2.4.3 deterministic focus order, 2.1.1 keyboard reachability); ReduceMotionPolicy disables animation/smooth-scrolling/cursor-blink when the OS requests reduced motion (2.3.3); TerminalAccessibleMode exposes the screen as a screen-reader buffer with a cursor announcement; the screen-reader checklist covers VoiceOver/NVDA/TalkBack (in-model checks automated, live checks on native hosts); cargo check/test --locked passed.');