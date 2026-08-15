#!/usr/bin/env node

// T112 contract: command snippets, variable hints, sensitive injection.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub const SECRET_MASK', 'pub enum VariableKind', 'pub struct SnippetVariable',
  'pub struct SnippetTemplate', 'pub fn variables_in_command', 'pub enum SnippetError',
  'pub struct RenderResult', 'pub struct SnippetEngine', 'pub fn render',
  'pub struct HistoryEntry', 'pub struct CommandHistory', 'pub fn record', 'pub fn contains',
  'pub struct VariableHints', 'pub fn resolve',
  'render_substitutes_and_masks_secrets', 'sensitive_values_never_enter_history',
  'template_injection_is_not_recursive', 'variable_validation_rejects_missing_and_unknown',
  'variables_in_command_are_parsed_in_order', 'variable_hints_resolve_by_prefix',
  'non_secret_render_is_not_marked_sensitive',
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
  console.error(`snippets contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('snippets contract valid: SnippetEngine::render substitutes {{variables}} in a single pass (a value containing {{...}} is inserted literally, so no template injection), produces a masked preview for secrets, and rejects missing/unknown variables; CommandHistory records only the masked preview so sensitive values never enter history (leak test passes); VariableHints resolves prefix-based autocomplete; template injection and history-leak tests pass; cargo check/test --locked passed.');