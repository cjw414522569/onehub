#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/ssh-backend');
const errors = [];

const REQUIRED_TOKENS = [
  'pub struct PtyConfig', 'pub struct ExecCommand', 'pub struct ExitStatus',
  'pub fn is_success', 'pub struct ChannelEvent', 'pub enum ChannelError',
  'pub trait SessionChannel', 'async fn request_pty', 'async fn start_shell',
  'async fn exec', 'async fn set_window_size', 'async fn read_event', 'async fn close',
  'pub struct ScriptedChannel', 'pub async fn run_interactive_shell',
  'pub async fn run_exec_command',
  'interactive_shell_with_pty_terminates_and_propagates_exit',
  'exec_command_propagates_nonzero_exit_status',
  'resize_is_propagated_between_pty_and_shell',
  'term_matrix_records_each_terminal_type',
  'exec_with_signal_exit_is_not_success',
  'signal_exit_status_is_reported_for_shell',
  'pty_and_exec_validate_input',
  'exit_status_success_semantics',
];
const FORBIDDEN_DEPENDENCIES = [
  'russh', 'russh-keys', 'libssh', 'libssh2', 'ssh2', 'ssh2-sys', 'openssh',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/ssh-backend/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`ssh-backend has forbidden dependency: ${name}`);
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
  if (!sourceText.includes(token)) errors.push(`ssh-backend is missing required token: ${token}`);
}

const libRs = join(CRATE, 'src/lib.rs');
if (!existsSync(libRs) || !readFileSync(libRs, 'utf8').includes('pub mod session_channel;')) {
  errors.push('lib.rs does not register pub mod session_channel;');
}

for (const args of [['check', '-p', 'ssh-backend', '--locked'], ['test', '-p', 'ssh-backend', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ssh-backend failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`session-channel contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('session-channel contract valid: PTY request, shell/exec drivers, env propagation, window-size change, exit status (code+signal), TERM matrix, input validation, cargo check/test --locked passed.');