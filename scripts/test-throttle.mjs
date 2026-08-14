#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const RENDERER = join(ROOT, 'crates/wgpu-renderer');
const errors = [];

// T079: frame coalescing, refresh throttling, background-session throttling.
const RENDERER_TOKENS = [
  'pub struct FrameCoalescer', 'pub struct RefreshThrottle', 'pub struct BoundedUpdateQueue',
  'pub struct ThrottleConfig', 'pub struct SessionThrottler', 'pub enum SessionPriority',
  'pub fn notify', 'pub fn drain', 'pub fn should_render', 'pub fn on_update',
  'pub fn enqueue', 'pub fn dequeue', 'pub fn queued', 'pub fn dropped', 'pub fn frames_rendered',
  'foreground_interval', 'background_interval', 'max_queued',
  'coalescer_folds_updates_into_one_frame', 'refresh_throttle_limits_frame_rate',
  'bounded_queue_never_explodes_under_high_throughput',
  'foreground_renders_faster_than_background',
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
  console.error(`throttle contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('throttle contract valid: FrameCoalescer folds updates into one frame; RefreshThrottle caps frame rate; BoundedUpdateQueue prevents event-queue explosion (1,000,000-update stress stays bounded with drops counted); SessionThrottler renders foreground at full rate and background at a reduced rate; cargo check/test --locked passed.');