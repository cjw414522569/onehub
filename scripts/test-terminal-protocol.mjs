#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/core-protocol');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum TerminalColor', 'pub enum UnderlineStyle', 'pub struct TerminalStyle',
  'pub struct Hyperlink', 'pub struct ImagePlaceholder', 'pub struct TerminalCell',
  'pub struct TerminalRow', 'pub struct CursorState', 'pub struct Extension',
  'pub struct TerminalSnapshot', 'pub struct TerminalDelta', 'pub enum DeltaOp',
  'pub struct TerminalBatch', 'pub enum TerminalMessage', 'pub struct TerminalProtocolVersion',
  'snapshot_golden_serialization_is_stable',
  'backward_compatible_old_record_parses_with_defaults',
  'forward_compatible_unknown_extension_is_ignored',
];
const FORBIDDEN_DEPENDENCIES = [
  'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui', 'windows-app-sdk',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'tokio', 'wgpu', 'harfbuzz',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/core-protocol/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`core-protocol has forbidden dependency: ${name}`);
}

const sourceFiles = [];
function collect(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collect(absolute);
    else if (entry.name.endsWith('.rs')) sourceFiles.push(absolute);
  }
}
if (existsSync(join(CRATE, 'src'))) collect(join(CRATE, 'src'));
const sourceText = sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`core-protocol is missing required token: ${token}`);
}

// Every render_op_kind from the terminal contract must be covered by DeltaOp.
const contract = JSON.parse(readFileSync(join(ROOT, 'protocol/terminal/terminal-contract-v1.json'), 'utf8'));
const renderKinds = contract.render_op_kinds ?? [];
for (const kind of renderKinds) {
  const capitalized = kind.charAt(0).toUpperCase() + kind.slice(1);
  if (!sourceText.includes(`${capitalized} {`) && !sourceText.includes(capitalized)) {
    errors.push(`DeltaOp must cover render_op_kind: ${kind}`);
  }
}

for (const args of [['check', '-p', 'core-protocol', '--locked'], ['test', '-p', 'core-protocol', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p core-protocol failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`terminal protocol contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`terminal protocol contract valid: snapshot/delta/style/hyperlink/image-placeholder, ${renderKinds.length} render_op_kinds covered, golden + forward/backward compat tests, cargo check/test --locked passed.`);