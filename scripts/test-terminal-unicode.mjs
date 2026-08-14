#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const PROTOCOL = join(ROOT, 'crates/core-protocol');
const PARSER = join(ROOT, 'crates/terminal-parser');
const errors = [];

// T064: locked Unicode version, configurable width policy, grapheme cluster
// segmentation (combining + ZWJ emoji), wide-character continuation cells in
// the screen model, and a golden snapshot covering CJK/combining/emoji.
const STATE_TOKENS = [
  'pub const UNICODE_VERSION', 'pub enum WidthPolicy', 'EastAsian', 'Legacy',
  'pub fn grapheme_clusters', 'pub fn char_width', 'pub fn cluster_width',
  'fn put_grapheme', 'wide_continuation', 'pub fn set_width_policy',
  'pub fn width_policy', 'break_wide_at',
  'unicode_version_is_locked_and_matches_tables', 'ascii_and_cjk_widths',
  'ambiguous_width_depends_on_policy', 'combining_and_emoji_cluster_widths',
  'wide_char_occupies_two_columns', 'combining_sequence_is_one_cell',
  'emoji_zwj_is_one_wide_cell', 'overwrite_breaks_wide_pair',
  'erase_continuation_breaks_wide_pair', 'width_policy_is_configurable',
];
const PROTOCOL_TOKENS = ['pub wide_continuation: bool', 'pub fn cluster', 'pub fn wide_continuation'];
const GOLDEN_TOKENS = ['golden_vttest_basic_snapshot', 'family emoji'];
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
checkCrateTokens(PROTOCOL, PROTOCOL_TOKENS, 'core-protocol');
checkCrateTokens(PARSER, GOLDEN_TOKENS, 'terminal-parser (golden)');

// terminal-state must pull the width tables and grapheme segmentation crates.
const stateManifest = readFileSync(join(STATE, 'Cargo.toml'), 'utf8');
for (const dep of ['unicode-width', 'unicode-segmentation']) {
  if (!stateManifest.includes(dep)) errors.push(`terminal-state is missing dependency: ${dep}`);
}

checkForbiddenDeps(STATE, 'terminal-state');
checkForbiddenDeps(PARSER, 'terminal-parser');

for (const args of [
  ['check', '-p', 'terminal-state', '--locked'],
  ['test', '-p', 'terminal-state', '--locked'],
  ['check', '-p', 'core-protocol', '--locked'],
  ['test', '-p', 'core-protocol', '--locked'],
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
  console.error(`terminal-unicode contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('terminal-unicode contract valid: UNICODE_VERSION locked to the unicode-width tables, configurable WidthPolicy (Unicode/EastAsian/Legacy), UAX #29 grapheme clusters (combining + ZWJ emoji), wide-continuation cells in TerminalCell/ScreenModel, golden covers CJK/combining/emoji, cargo check/test --locked passed.');