#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const PARSER = join(ROOT, 'crates/terminal-parser');
const errors = [];

// T063: primary/alternate screen model, cursor + saved cursor, scroll region,
// DEC/ANSI mode state, SGR; parser vocabulary (incl. SetScrollRegion) owned by
// terminal-state; deterministic vttest-basic golden snapshot in terminal-parser.
const STATE_TOKENS = [
  'pub struct ScreenModel', 'pub struct ScreenBuffer', 'pub struct Modes',
  'pub enum ParseEvent', 'pub struct ParserDiagnostic', 'pub struct ParseBatch',
  'pub trait TerminalParser', 'SetScrollRegion', 'alternate_screen',
  'scroll_top', 'scroll_bottom', 'saved_row', 'saved_col', 'pending_wrap',
  'pub fn apply_batch', 'pub fn apply_event', 'pub fn snapshot', 'pub fn resize',
  'text_writes_and_wraps_at_right_edge', 'autowrap_off_keeps_cursor_at_right_edge',
  'linefeed_scrolls_within_region', 'origin_mode_positions_cursor_in_region',
  'erase_display_and_line', 'sgr_basic_applies_style_to_cells',
  'alternate_screen_switch_preserves_primary', 'cursor_move_respects_scroll_region',
  'title_and_modes_are_recorded', 'modes_defaults',
];
const GOLDEN_TOKENS = ['golden_vttest_basic_snapshot', 'vttest-basic'];
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
if (!existsSync(join(STATE, 'README.md'))) errors.push('Missing crates/terminal-state/README.md');
checkCrateTokens(STATE, STATE_TOKENS, 'terminal-state');
checkCrateTokens(PARSER, GOLDEN_TOKENS, 'terminal-parser (golden)');
if (!existsSync(join(PARSER, 'tests/golden/vttest-basic.json'))) {
  errors.push('Missing golden file crates/terminal-parser/tests/golden/vttest-basic.json');
}
checkForbiddenDeps(STATE, 'terminal-state');
checkForbiddenDeps(PARSER, 'terminal-parser');

for (const args of [
  ['check', '-p', 'terminal-state', '--locked'],
  ['test', '-p', 'terminal-state', '--locked'],
  ['check', '-p', 'terminal-parser', '--locked'],
  ['test', '-p', 'terminal-parser', '--locked'],
]) {
  const crate = args[1] === 'terminal-state' ? 'terminal-state' : 'terminal-parser';
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`terminal-screen contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('terminal-screen contract valid: ScreenModel primary/alternate buffers, cursor + saved cursor, scroll region, DEC/ANSI modes, SGR; ParseEvent vocabulary (incl. SetScrollRegion) owned by terminal-state; deterministic vttest-basic golden snapshot; no external terminal engine dependency; cargo check/test --locked passed.');