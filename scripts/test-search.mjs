#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const errors = [];

// T071: search, regex search, and result navigation over scrollback + screen.
// Chunked, cancellable (AtomicBool), non-blocking; million-line benchmark.
const STATE_TOKENS = [
  'pub struct SearchQuery', 'pub struct SearchResult', 'pub struct SearchBuffer',
  'pub struct SearchSession', 'pub struct SearchNavigation', 'pub fn literal',
  'pub fn step', 'pub fn cancel', 'pub fn was_cancelled', 'pub fn is_done',
  'pub fn cancel_token', 'pub fn run', 'pub fn lines_searched', 'pub fn next_result', 'pub fn prev_result',
  'regex::RegexBuilder', 'AtomicBool',
  'literal_search_finds_all_occurrences', 'regex_search_and_offsets',
  'case_insensitive_literal', 'chunked_search_is_incremental_and_non_blocking',
  'cancellable_search_stops_early', 'million_line_search_benchmark_and_cancellation',
  'navigation_cycles_through_results',
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

// terminal-state must pull the regex crate.
const manifest = readFileSync(join(STATE, 'Cargo.toml'), 'utf8');
if (!manifest.includes('regex = "1"')) errors.push('terminal-state is missing the regex dependency');

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
  console.error(`search contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('search contract valid: SearchSession searches scrollback+screen in bounded chunks with AtomicBool cancellation (non-blocking, cancellable), plain-text and regex queries (RegexBuilder), SearchNavigation cycles results; million-line benchmark bounded; cargo check/test --locked passed.');