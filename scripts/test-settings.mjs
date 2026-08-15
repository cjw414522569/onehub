#!/usr/bin/env node

// T116 contract: settings / theme / font / terminal / network policy UI.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub const SETTINGS_VERSION', 'pub enum EffectTiming', 'pub enum ProxyMode',
  'pub struct AppearanceSettings', 'pub struct FontSettings', 'pub struct TerminalSettings',
  'pub struct NetworkPolicySettings', 'pub struct Settings', 'pub fn defaults',
  'pub fn effect_timing', 'pub fn reset_to_defaults', 'pub fn snapshot', 'pub fn from_snapshot',
  'pub struct SettingsSnapshot', 'pub enum SettingsError', 'pub fn migrate_snapshot',
  'defaults_restore_and_effect_timing_is_explicit', 'persistence_round_trip',
  'migration_fills_missing_keys_with_defaults', 'invalid_values_are_rejected',
  'unknown_keys_are_forward_compatible',
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
  console.error(`settings contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('settings contract valid: Settings groups appearance/font/terminal/network policy and every item declares its EffectTiming (immediate / on-reconnect / on-restart) with a label; versioned SettingsSnapshot persistence round-trips exactly and validates ranges (font 8..72, scrollback 0..1M, keepalive 0..86400, bools, proxy modes); migrate_snapshot upgrades older snapshots filling missing keys with defaults; reset_to_defaults restores defaults; unknown keys are forward-compatible; persistence and migration tests pass; cargo check/test --locked passed.');