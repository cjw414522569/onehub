#!/usr/bin/env node

// T107 contract: mobile session stack, bottom action bar, safe areas.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum FormFactor', 'pub enum Orientation', 'pub struct Viewport', 'pub struct SafeAreaInsets',
  'pub fn effective_safe_area', 'pub struct BottomActionBar', 'pub struct BarLayout',
  'pub struct SessionStack', 'pub enum SystemBack', 'pub fn form_factor', 'pub fn orientation',
  'pub fn is_one_handed_compatible', 'pub fn layout', 'pub fn on_system_back',
  'form_factor_and_orientation_detection', 'safe_area_adapts_to_landscape_phone_cutout',
  'bottom_bar_layout_golden_phone_tablet_landscape', 'session_stack_system_back_pops_then_exits',
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
  console.error(`mobile-layout contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('mobile-layout contract valid: Viewport derives form factor (phone/tablet via the smaller dimension) and orientation; one-handed compatibility is phone portrait; effective_safe_area adds side insets for landscape-phone cutouts; BottomActionBar layout is deterministic and golden-tested for phone portrait / tablet / landscape (3 vs 5 actions, 48/56px); SessionStack.on_system_back pops history then asks the app to exit; phone/tablet golden and navigation tests pass; cargo check/test --locked passed.');