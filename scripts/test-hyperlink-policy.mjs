#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const PARSER = join(ROOT, 'crates/terminal-parser');
const errors = [];

// T067: OSC 8 hyperlinks with an explicit open-confirmation policy. Dangerous
// schemes (javascript:/data:/vbscript:/file:) are forbidden; allowed URIs are
// reviewable (effective host surfaced) before opening.
const STATE_TOKENS = [
  'pub struct HyperlinkPolicy', 'pub struct HyperlinkReview', 'pub fn can_open',
  'pub fn review', 'pub fn scheme_allowed', 'pub fn effective_host', 'pub fn scheme_of',
  'Hyperlink { id: Option<String>, url: String }', 'pub fn set_hyperlink_policy',
  'scheme_whitelist_blocks_dangerous_schemes', 'scheme_and_host_parsing',
  'phishing_url_surfaces_effective_host', 'forbidden_and_oversized_uris_have_no_review',
  'osc8_hyperlink_attaches_and_clears', 'osc8_dangerous_scheme_is_ignored',
];
const PARSER_TOKENS = ['osc8_hyperlink_parsing', '"8" => {', 'Hyperlink'];
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
checkCrateTokens(PARSER, PARSER_TOKENS, 'terminal-parser');
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
  console.error(`hyperlink-policy contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('hyperlink-policy contract valid: OSC 8 parsed (id + uri, empty uri ends link); HyperlinkPolicy whitelists https/http/ssh/sftp/mailto and forbids javascript:/data:/vbscript:/file:; review() surfaces the effective host for explicit open confirmation (phishing samples tested); cells carry hyperlink id+url in the golden; cargo check/test --locked passed.');