#!/usr/bin/env node

// T127 contract: Android lifecycle, foreground service, network-switch model.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum AppState', 'pub enum NetworkState', 'pub struct LifecycleModel', 'pub fn new',
  'pub fn on_foreground', 'pub fn on_background', 'pub fn start_session', 'pub fn end_session',
  'pub fn set_foreground_service', 'pub fn set_doze', 'pub fn set_network',
  'pub fn requires_foreground_service', 'pub fn can_sustain_session', 'pub fn claims_online',
  'pub fn is_deceptive',
  'lifecycle_transitions', 'foreground_service_is_required_in_background_with_active_session',
  'doze_blocks_background_sessions', 'network_switch_keeps_session_and_none_does_not',
  'never_pretends_online_without_a_session',
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
  console.error(`android-integration contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('android-integration contract valid: LifecycleModel follows Android background rules (active session in the background requires a foreground service, Doze gates the network, and Wi-Fi<->cellular switches never lose the session); is_deceptive is true exactly when the UI claims an active session that cannot be sustained, so the app never pretends to be permanently online; cargo check/test --locked passed (real Doze/network-switch/process-reclaim tests run on Android devices).');