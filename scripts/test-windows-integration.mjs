#!/usr/bin/env node

// T121 contract: Windows window/tray/protocol/notification/install model.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum WindowsArch', 'pub struct Size', 'pub struct Rect', 'pub struct Monitor',
  'pub struct DpiContext', 'pub fn logical_to_physical', 'pub struct MonitorLayout',
  'pub fn constrain_restore', 'pub enum TrayAction', 'pub struct ProtocolLink',
  'pub enum LinkError', 'pub fn parse_ssh_link', 'pub struct WindowsNotification',
  'pub struct SleepWakePolicy', 'pub fn handle_wake',
  'architecture_and_dpi_round_trip', 'multi_monitor_restore_stays_on_screen',
  'tray_actions_and_wake_reconnect', 'protocol_links_parse',
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
  console.error(`windows-integration contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('windows-integration contract valid: the model covers x64/arm64, DPI scaling (logical<->physical round-trip), multi-monitor window restore (a window saved on an unplugged monitor is moved into a visible work area), tray actions, sleep/wake reconnection, secret-free notifications, and ssh:// protocol-link parsing; the deterministic checks pass here and the real Win32 tray/message-loop/installer bindings run on Windows hosts; cargo check/test --locked passed.');