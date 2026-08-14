#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const RENDERER = join(ROOT, 'crates/wgpu-renderer');
const errors = [];

// T078: cursor/selection/decoration/link/search layer compositing.
const RENDERER_TOKENS = [
  'pub enum Layer', 'pub struct CompositeState', 'pub struct FramePlan', 'pub struct FrameTimeline',
  'pub struct DecorationRect', 'pub struct LinkRect', 'pub struct SearchMatchRect',
  'pub fn set_base', 'pub fn set_cursor', 'pub fn set_selection', 'pub fn set_decorations',
  'pub fn set_links', 'pub fn set_search_matches', 'pub fn plan_frame', 'pub fn selection_rects',
  'pub fn selected_text', 'pub fn record',
  'selection_change_does_not_touch_base', 'unchanged_frame_is_stable',
  'cursor_blink_only_redraws_cursor', 'timeline_records_per_frame_redraws',
  'search_layer_is_independent', 'selection_text_extraction',
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
  console.error(`composite contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('composite contract valid: CompositeState tracks cursor/selection/decoration/link/search overlays with per-layer dirty flags; changing one layer never re-lays out the base grid; plan_frame returns exactly the redrawn layers and stable frames redraw nothing (no flicker); FrameTimeline records per-frame plans; cargo check/test --locked passed.');