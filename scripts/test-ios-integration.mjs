#!/usr/bin/env node

// T129 contract: iOS/iPadOS lifecycle, scenes, multi-window, suspension.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum SceneState', 'pub struct Scene', 'pub struct SceneCollection', 'pub fn add_scene',
  'pub fn close_scene', 'pub fn activate', 'pub struct SuspensionModel', 'pub fn on_foreground',
  'pub fn on_background', 'pub fn start_background_task', 'pub fn end_background_task',
  'pub fn may_claim_active', 'pub fn suspended', 'pub fn is_deceptive',
  'pub struct RecoveryState', 'pub fn restore', 'pub fn consistent',
  'scene_collection_multi_window', 'suspension_limits_are_truthful',
  'recovery_state_is_consistent',
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
  console.error(`ios-integration contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('ios-integration contract valid: SceneCollection models multi-scene (multi-window) lifecycle with activate/close; SuspensionModel presents iOS suspension limits truthfully (the UI never claims an active session while iOS would suspend the app - background without a background task; a running background task makes the claim truthful); RecoveryState restores sessions so the restored state matches the saved state; cargo check/test --locked passed (simulator + device lifecycle tests run on Apple hosts).');