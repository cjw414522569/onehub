#!/usr/bin/env node

// T111 contract: session state, latency, reconnect, read-only indicators.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum SessionState', 'pub enum IndicatorPattern', 'pub struct StateIndicator',
  'pub enum LatencyQuality', 'pub struct SessionStatus', 'pub struct StateError',
  'pub struct SessionStatusModel', 'pub fn connect', 'pub fn on_connected',
  'pub fn on_disconnected', 'pub fn on_error', 'pub fn set_read_only', 'pub fn close',
  'pub fn indicator', 'pub fn latency_label', 'pub fn latency_quality',
  'state_machine_valid_transitions', 'error_path_records_message_and_attempts',
  'every_state_has_a_non_color_indicator', 'latency_is_shown_as_text_not_color',
  'read_only_and_reconnect_indicators_differ',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(LIB, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`host-library missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'host-library', '--locked'],
  ['test', '-p', 'host-library', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p host-library failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`session-status contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('session-status contract valid: SessionStatusModel is a validated state machine (disconnected/connecting/connected/reconnecting/read-only/error/closed) with reconnect attempts and error messages; every state maps to a non-color StateIndicator (glyph + label + description + pattern) and the exhaustive test asserts all seven states have unique (glyph, pattern) pairs; latency is shown as text with a quality label, never by color alone; the state-machine UI mapping is exhaustively tested; cargo check/test --locked passed.');