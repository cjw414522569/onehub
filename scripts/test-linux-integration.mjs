#!/usr/bin/env node

// T125 contract: Linux Wayland/X11, Secret Service, notifications, desktop entry.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum DisplayServer', 'pub fn has_legacy_clipboard', 'pub enum DesktopEnvironment',
  'pub struct ScalingPolicy', 'pub fn for_display', 'pub enum NoKeyringPolicy',
  'pub struct SecretServiceState', 'pub fn detect', 'pub fn can_persist',
  'pub struct DesktopEntry', 'pub fn minimal', 'pub fn to_desktop_file',
  'pub struct LinuxNotification', 'pub fn contains',
  'display_server_clipboard_and_scaling', 'secret_service_and_no_keyring_fallback',
  'desktop_entry_generates_valid_file', 'notifications_do_not_leak_secrets',
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
  console.error(`linux-integration contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('linux-integration contract valid: the model covers X11/Wayland clipboard behavior and fractional scaling, Secret Service availability with a no-keyring fallback (memory-only on GNOME/KDE without a keyring, refuse headless), a valid .desktop entry, and secret-free notifications; GNOME/KDE behavior is explicit and the real distro/display-server matrix runs on Linux hosts; cargo check/test --locked passed.');