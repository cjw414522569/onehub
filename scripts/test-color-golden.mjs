#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const PARSER = join(ROOT, 'crates/terminal-parser');
const errors = [];

// T065: 16/256/truecolor, inverse/dim combinations, underline styles
// (including `4:N` colon sub-parameters), configurable palette with indexed
// color resolution, and a color-matrix golden snapshot.
const STATE_TOKENS = [
  'pub struct Palette', 'pub struct Rgb', 'pub fn resolve_indexed', 'CUBE_LEVELS',
  'default_fg', 'default_bg', 'Sgr { params: Vec<Vec<u16>> }',
  'UnderlineStyle::Single', 'UnderlineStyle::Double', 'UnderlineStyle::Curly',
  'UnderlineStyle::Dotted', 'UnderlineStyle::Dashed',
  'sgr_256_color_and_truecolor', 'sgr_bright_16_colors',
  'sgr_inverse_dim_combination', 'sgr_underline_styles',
  'default_palette_has_expected_anchors', 'cube_and_grayscale_resolution',
  'resolve_covers_all_color_kinds',
];
const GOLDEN_TOKENS = ['golden_color_matrix_snapshot', 'color-matrix.json'];
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
checkCrateTokens(PARSER, GOLDEN_TOKENS, 'terminal-parser (golden)');
if (!existsSync(join(PARSER, 'tests/golden/color-matrix.json'))) {
  errors.push('Missing golden file crates/terminal-parser/tests/golden/color-matrix.json');
}
checkForbiddenDeps(STATE, 'terminal-state');
checkForbiddenDeps(PARSER, 'terminal-parser');

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
  console.error(`color-golden contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('color-golden contract valid: 16/256/truecolor SGR, inverse/dim combinations, underline styles incl. 4:N sub-params, configurable Palette with xterm indexed resolution (cube + grayscale), color-matrix golden snapshot, cargo check/test --locked passed.');