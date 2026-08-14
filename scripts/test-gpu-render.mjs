#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const RENDERER = join(ROOT, 'crates/wgpu-renderer');
const errors = [];

// T077: batched GPU terminal drawing on a single native surface. No per-cell
// UI controls; per-frame draw-call budget enforced; 4K grid fits the budget.
const RENDERER_TOKENS = [
  'pub struct DrawBudget', 'pub struct DrawCall', 'pub struct RenderPlan', 'pub struct FrameStats',
  'pub struct RenderSurface', 'pub fn build_plan', 'pub fn merge_to_budget',
  'pub fn frame_stats', 'pub fn begin_frame', 'max_draw_calls', 'max_instances_per_call',
  'instances', 'under_budget',
  'plan_groups_by_glyph_and_style', 'pathological_frame_stays_within_budget',
  'four_k_grid_benchmark_fits_budget', 'merge_keeps_instances_when_merging',
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
  // Forbid per-cell UI controls: no CellWidget / cell-widget API may exist.
  if (/CellWidget|cell_widget|per_cell_control/i.test(sourceText)) {
    errors.push(`${label} must not expose per-cell UI controls`);
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
  console.error(`gpu-render contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gpu-render contract valid: single RenderSurface entry (no per-cell UI controls); build_plan groups cells into instanced DrawCalls; DrawBudget bounds per-frame draw calls with merge_to_budget fallback; simulated 4K grid (240x67) fits the default budget with all cells instanced; cargo check/test --locked passed.');