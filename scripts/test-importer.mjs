#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SSH = join(ROOT, 'crates/ssh-backend');
const errors = [];

// T091: OpenSSH config/known_hosts/key-metadata importer.
const SSH_TOKENS = [
  'pub struct ParsedDirective', 'pub struct ConfigParseResult', 'pub struct KnownHostsLine',
  'pub struct KeyMetadata', 'pub fn parse_config', 'pub fn parse_known_hosts',
  'pub fn inspect_key', 'fn split_line',
  'config_corpus_parses_include_match_proxyjump', 'unknown_directives_produce_warnings',
  'known_hosts_corpus_parses', 'key_inspection_never_copies_private_key',
  'empty_config_is_clean',
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

if (!existsSync(join(SSH, 'Cargo.toml'))) errors.push('Missing crates/ssh-backend/Cargo.toml');
checkCrateTokens(SSH, SSH_TOKENS, 'ssh-backend');
checkForbiddenDeps(SSH, 'ssh-backend');

for (const args of [
  ['check', '-p', 'ssh-backend', '--locked'],
  ['test', '-p', 'ssh-backend', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 300000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`importer contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('importer contract valid: parse_config reports Host/HostName/User/Port/IdentityFile/ProxyJump/Include/Match/etc. with per-directive lines and warnings for unknown directives; parse_known_hosts reports each line (incl. hashed |1|salt|host|); inspect_key reads only private-key header/size and fingerprints a public .pub sibling (private key never copied); OpenSSH config corpus tests pass; cargo check/test --locked passed.');