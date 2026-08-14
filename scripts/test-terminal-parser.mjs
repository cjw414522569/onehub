#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/terminal-parser');
const errors = [];

// T062 keeps its fragmentation/memory-bound contract. The event vocabulary
// (ParseEvent/ParserDiagnostic/ParseBatch/TerminalParser) is owned by
// terminal-state (T063) and re-exported here, so the contract asserts the
// re-export plus the new SetScrollRegion event.
const REQUIRED_TOKENS = [
  'pub struct BoundedByteStreamParser',
  'pub use terminal_state::parser::{ParseBatch, ParseEvent, ParserDiagnostic, TerminalParser}',
  'fn feed(&mut self', 'fn finish(&mut self',
  'pub fn with_caps', 'pub fn pending_len', 'MAX_UTF8_LEN', 'DEFAULT_MAX_SEQUENCE_LEN',
  'DEFAULT_MAX_TEXT_LEN', 'SetScrollRegion',
  'whole_equals_fragmented', 'fragmented_utf8_across_chunks', 'fragmented_csi_across_chunks',
  'fragmented_osc_across_chunks', 'basic_event_sequence', 'invalid_utf8_is_diagnosed_and_replaced',
  'malicious_input_memory_is_bounded', 'oversized_sequence_is_diagnosed_and_bounded',
  'finish_reports_truncated_sequence', 'text_buffer_is_capped_and_flushed',
];
const FORBIDDEN_DEPENDENCIES = ['russh', 'libssh', 'ssh2', 'openssh', 'vt-parser'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/terminal-parser/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`terminal-parser has forbidden dependency: ${name}`);
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
  if (!sourceText.includes(token)) errors.push(`terminal-parser is missing required token: ${token}`);
}

for (const args of [['check', '-p', 'terminal-parser', '--locked'], ['test', '-p', 'terminal-parser', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p terminal-parser failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`terminal-parser contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('terminal-parser contract valid: bounded fragmentation-safe UTF-8/CSI/OSC pipeline, whole==fragmented property, malicious-input memory bound, oversized-sequence diagnostics, vocabulary re-exported from terminal-state incl. SetScrollRegion, cargo check/test --locked passed.');