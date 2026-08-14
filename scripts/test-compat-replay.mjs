#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const PARSER = join(ROOT, 'crates/terminal-parser');
const errors = [];

// T082: recorded-replay compatibility regression set (vim/tmux/screen/htop/
// fzf/lazygit). The full interactive vttest/esctest suites need a real PTY
// and are blocked_environment; the deterministic recorded scripts lock the
// state transitions those apps rely on.
const REPLAY_TOKENS = [
  'fn vim_replay', 'fn tmux_replay', 'fn screen_replay', 'fn htop_replay',
  'fn fzf_replay', 'fn lazygit_replay', 'fn corpus_manifest_is_registered',
  'fn replay', 'fn flatten',
];
const FORBIDDEN_DEPENDENCIES = ['vt-parser', 'wezterm-term', 'alacritty_terminal'];

function checkTokens(crateDir, tokens, label) {
  const files = [];
  function collect(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const absolute = join(dir, entry.name);
      if (entry.isDirectory()) collect(absolute);
      else if (entry.name.endsWith('.rs')) files.push(absolute);
    }
  }
  if (existsSync(join(crateDir, 'tests'))) collect(join(crateDir, 'tests'));
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

if (!existsSync(join(PARSER, 'tests/compat_replay.rs'))) errors.push('Missing tests/compat_replay.rs');
if (!existsSync(join(PARSER, 'tests/compat-corpus.json'))) errors.push('Missing tests/compat-corpus.json');
checkTokens(PARSER, REPLAY_TOKENS, 'terminal-parser');
checkForbiddenDeps(PARSER, 'terminal-parser');

const manifest = JSON.parse(readFileSync(join(PARSER, 'tests/compat-corpus.json'), 'utf8').replace(/^\uFEFF/, ''));
const apps = manifest.corpus.map((entry) => entry.app);
for (const app of ['vim', 'tmux', 'screen', 'htop', 'fzf', 'lazygit']) {
  if (!apps.includes(app)) errors.push(`compat corpus is missing ${app}`);
}

for (const args of [
  ['check', '-p', 'terminal-parser', '--locked'],
  ['test', '-p', 'terminal-parser', '--locked'],
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
  console.error(`compat-replay contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('compat-replay contract valid: recorded vim/tmux/screen/htop/fzf/lazygit scripts replay through parser+model with clean diagnostics and expected markers (alt screen, status lines, box drawing, highlights); corpus manifest registered; cargo check/test --locked passed.');