#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const STATE = join(ROOT, 'crates/terminal-state');
const PARSER = join(ROOT, 'crates/terminal-parser');
const errors = [];

// T066: OSC title / working directory / notifications with a security policy.
// Untrusted sequences cannot bypass the title/notification policy: payloads
// are gated, control characters stripped, and lengths capped.
const STATE_TOKENS = [
  'pub struct OscPolicy', 'pub struct Notification', 'pub fn sanitize_title',
  'pub fn sanitize_working_directory', 'pub fn sanitize_notification',
  'pub allow_notifications', 'WorkingDirectory', 'pub fn take_notification',
  'pub fn set_osc_policy', 'pub fn working_directory',
  'title_is_sanitized_and_capped', 'title_denied_when_policy_blocks',
  'working_directory_is_sanitized', 'notifications_denied_by_default',
  'notifications_sanitized_when_allowed', 'osc_title_and_working_directory',
  'osc_notification_policy_gating', 'osc_untitled_sequences_cannot_bypass_policy',
];
const PARSER_TOKENS = [
  'fn parse_osc', 'osc_working_directory_and_notifications',
  'osc_terminated_by_st_is_finalized', 'osc_st', 'WorkingDirectory',
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
  console.error(`osc-policy contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('osc-policy contract valid: OSC 0/2 title, OSC 7 working directory, OSC 9/777 notifications parsed (BEL and ST terminated); OscPolicy gates title/working-directory/notifications (notifications denied by default), strips control chars, caps lengths; golden captures working_directory; cargo check/test --locked passed.');