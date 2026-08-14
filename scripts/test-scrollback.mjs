#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const errors = [];

// T069: configurable scrollback ring buffer with an explicit memory bound and
// a disk dump policy that is off by default (sensitive dumps).
const STATE_TOKENS = [
  'pub struct Scrollback', 'pub struct ScrollbackConfig', 'pub struct ScrollbackDumpPolicy',
  'pub fn push', 'pub fn set_max_lines', 'pub fn len', 'pub fn lines_dropped',
  'pub fn permitted', 'pub fn dump', 'max_lines',
  'pub fn scrollback_len', 'pub fn set_scrollback_config', 'pub fn set_scrollback_dump_policy',
  'pub fn dump_scrollback', 'scrollback_start',
  'ring_buffer_bounds_memory_explicitly', 'million_line_scroll_memory_benchmark_is_bounded',
  'zero_capacity_disables_capture', 'config_controls_capacity',
  'dump_is_off_by_default_and_bounded_when_enabled',
  'scrollback_captures_scrolled_lines', 'alternate_screen_has_no_scrollback',
  'scrollback_dump_off_by_default',
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
  if (existsSync(join(crateDir, 'tests'))) collectRs(join(crateDir, 'tests'), files);
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

if (!existsSync(join(STATE, 'Cargo.toml'))) errors.push('Missing crates/terminal-state/Cargo.toml');
checkCrateTokens(STATE, STATE_TOKENS, 'terminal-state');
checkForbiddenDeps(STATE, 'terminal-state');

for (const args of [
  ['check', '-p', 'terminal-state', '--locked'],
  ['test', '-p', 'terminal-state', '--locked'],
  ['check', '-p', 'terminal-parser', '--locked'],
  ['test', '-p', 'terminal-parser', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`scrollback contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('scrollback contract valid: bounded ring buffer with explicit max_lines (million-line benchmark stays within the bound), configurable capacity with eviction, primary-screen-only capture, disk dump policy off by default and length-bounded when enabled, snapshot scrollback_start wired; cargo check/test --locked passed.');