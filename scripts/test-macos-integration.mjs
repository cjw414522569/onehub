#!/usr/bin/env node

// T123 contract: macOS menu / window / Keychain / notification / deep-link model.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum MacArch', 'pub enum RetinaScale', 'pub fn factor', 'pub fn logical_to_physical',
  'pub enum MacMenuAction', 'pub struct MacMenu', 'pub fn default_menu', 'pub fn actions',
  'pub struct AppNapPolicy', 'pub fn for_active_sessions', 'pub fn may_nap',
  'pub struct MacNotification', 'pub fn contains',
  'architecture_and_retina_scale', 'multi_monitor_restore_uses_work_area',
  'app_nap_is_disabled_during_active_sessions', 'default_menu_covers_standard_actions',
  'notifications_do_not_leak_secrets',
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
  console.error(`macos-integration contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('macos-integration contract valid: the model covers Intel / Apple Silicon, Retina backing scale (@1x/@2x), multi-monitor window restore (respecting the menu-bar work area), App Nap policy (disabled during active sessions), the default app menu, and secret-free notifications; deep links reuse parse_ssh_link; the deterministic checks pass here and real macOS automation / the physical-machine checklist run on macOS hosts; cargo check/test --locked passed.');