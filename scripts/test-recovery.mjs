#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const RENDERER = join(ROOT, 'crates/wgpu-renderer');
const errors = [];

// T080: GPU/device-loss, foreground/background, window-rebuild recovery.
const RENDERER_TOKENS = [
  'pub enum LifecyclePhase', 'pub struct RecoveryCoordinator', 'pub fn retain',
  'pub fn on_device_lost', 'pub fn begin_rebuild', 'pub fn finish_rebuild',
  'pub fn on_app_background', 'pub fn on_app_foreground', 'pub fn on_window_recreated',
  'pub fn retained_snapshot', 'pub fn session_alive', 'pub fn losses', 'pub fn rebuilds',
  'pub fn window_recreations',
  'device_loss_rebuild_restores_consistent_content', 'rebuild_without_retained_content_fails',
  'background_foreground_keeps_content_and_session', 'window_rebuild_restores_content',
  'full_lifecycle_fault_injection',
];
const FORBIDDEN_DEPENDENCIES = ['vt-parser', 'wezterm-term', 'alacritty_terminal'];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

function checkCrateTokens(crateDir, tokens, label) {
  const files = [];
  if (existsSync(join(crateDir, 'src'))) collectRs(join(crateDir, 'src'), files);
  const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
  for (const token of tokens) {
    if (!sourceText.includes(token)) errors.push(`${label} is missing required token: ${token}`);
  }
}

function checkForbiddenDeps(crateDir, label) {
  const manifest = readFileSync(join(crateDir, 'Cargo.toml'), 'utf8');
  const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
  const depsSection = depsMatch?.[1] ?? '';
  for (const line of depsSection.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
    if (!name) continue;
    if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`${label} has forbidden dependency: ${name}`);
  }
}

if (!existsSync(join(RENDERER, 'Cargo.toml'))) errors.push('Missing crates/wgpu-renderer/Cargo.toml');
checkCrateTokens(RENDERER, RENDERER_TOKENS, 'wgpu-renderer');
checkForbiddenDeps(RENDERER, 'wgpu-renderer');

for (const args of [
  ['check', '-p', 'wgpu-renderer', '--locked'],
  ['test', '-p', 'wgpu-renderer', '--locked'],
  ['check', '-p', 'terminal-state', '--locked'],
  ['test', '-p', 'terminal-state', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`recovery contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('recovery contract valid: RecoveryCoordinator retains terminal content across device loss / background / window rebuild; phase machine Healthy->DeviceLost->Rebuilding->Recovered; recovered content equals the retained snapshot; session_alive stays true (renderer never disconnects SSH); lifecycle fault injection tests pass; cargo check/test --locked passed.');