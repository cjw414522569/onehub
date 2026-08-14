#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const errors = [];

// T068: input protocol encoders — bracketed paste, focus events, mouse
// reporting (X10/SGR), and keyboard protocols (xterm / modifyOtherKeys /
// kitty) with negotiation from the kitty probe reply.
const STATE_TOKENS = [
  'pub fn encode_paste', 'pub fn encode_focus', 'pub fn encode_mouse', 'pub fn encode_key',
  'pub enum MouseMode', 'pub enum MouseEncoding', 'pub enum KeyboardProtocol',
  'pub fn kitty_probe', 'pub fn from_kitty_reply', 'pub fn kitty_bits', 'pub fn xterm_bits',
  'pub struct KeyEvent', 'pub struct MouseEvent', 'pub struct Modifiers',
  'pub enum Key', 'pub enum MouseButton', 'pub enum MouseAction',
  'paste_is_wrapped_only_when_bracketed', 'focus_events_report_only_when_enabled',
  'sgr_mouse_encoding', 'x10_mouse_encoding', 'xterm_key_encoding',
  'modify_other_keys_encoding', 'kitty_key_encoding', 'keyboard_protocol_negotiation',
  'input_protocol_modes_wire',
  'pub mouse_mode: MouseMode', 'pub mouse_sgr: bool', 'pub focus_events: bool',
  'pub keyboard_protocol: KeyboardProtocol',
  '1004 => self.modes.focus_events', '1006 => self.modes.mouse_sgr',
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
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`input-protocol contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('input-protocol contract valid: bracketed paste (200~/201~), focus reports (?1004), mouse encoders (X10 + SGR with modifier bits), keyboard encoders (xterm, modifyOtherKeys 27;mod;code~, kitty CSI code:mod[:repeat] u) with kitty probe negotiation; mode wiring for ?1000/?1002/?1003/?1004/?1006; cargo check/test --locked passed.');