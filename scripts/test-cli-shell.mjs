#!/usr/bin/env node

// T145 contract: shell completion (PowerShell/bash/zsh/fish), man page, and
// versioned machine-readable JSON output.

import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CLI = join(ROOT, 'apps/cli');
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Build + unit tests (JSON output module).
const check = run('cargo', ['check', '-p', 'cli', '--locked']);
if (check.status !== 0) errors.push(`cargo check -p cli failed:\n${check.stdout}\n${check.stderr}`);
const test = run('cargo', ['test', '-p', 'cli', '--locked']);
if (test.status !== 0) errors.push(`cargo test -p cli failed:\n${test.stdout}\n${test.stderr}`);
const build = run('cargo', ['build', '-p', 'cli', '--locked']);
if (build.status !== 0) errors.push(`cargo build -p cli failed:\n${build.stdout}\n${build.stderr}`);
const exe = join(ROOT, 'target/debug/ssh-cli.exe');

// 2. Completion scripts: each shell has a real, loadable completion file
//    covering the command surface.
const completionChecks = [
  { file: 'ssh-cli.bash', marker: 'complete -F _ssh_cli ssh-cli' },
  { file: 'ssh-cli.zsh', marker: '#compdef ssh-cli' },
  { file: 'ssh-cli.fish', marker: 'complete -c ssh-cli' },
  { file: 'ssh-cli.ps1', marker: 'Register-ArgumentCompleter' },
];
for (const { file, marker } of completionChecks) {
  const path = join(CLI, 'completions', file);
  if (!existsSync(path)) {
    errors.push(`missing completion script: ${file}`);
    continue;
  }
  const text = readFileSync(path, 'utf8');
  if (!text.includes(marker)) errors.push(`${file} missing marker: ${marker}`);
  for (const sub of ['config', 'cap', '--version', '--help', '--json']) {
    if (!text.includes(sub)) errors.push(`${file} missing command surface token: ${sub}`);
  }
}

// 3. Man page: standard roff sections + stable exit codes.
const manPath = join(CLI, 'docs/ssh-cli.1');
if (!existsSync(manPath)) {
  errors.push('missing man page ssh-cli.1');
} else {
  const man = readFileSync(manPath, 'utf8');
  for (const section of ['.TH SSH-CLI', '.SH SYNOPSIS', '.SH DESCRIPTION', '.SH OPTIONS', '.SH EXIT STATUS', '.SH JSON OUTPUT']) {
    if (!man.includes(section)) errors.push(`man page missing section: ${section}`);
  }
  for (const code of ['0', '1', '2', '3', '4', '5']) {
    if (!man.includes(`.B ${code}`)) errors.push(`man page missing exit status ${code}`);
  }
}

// 4. Versioned JSON output: the binary emits schema_version 1 payloads.
function jsonOf(args) {
  const res = spawnSync(exe, args, { encoding: 'utf8', timeout: 60000 });
  try {
    return { parsed: JSON.parse(res.stdout), status: res.status, stdout: res.stdout };
  } catch {
    errors.push(`ssh-cli ${args.join(' ')} did not emit JSON: ${res.stdout}`);
    return null;
  }
}

const version = jsonOf(['--json', '--version']);
if (version) {
  if (version.parsed.schema_version !== 1) errors.push('version payload schema_version != 1');
  if (version.parsed.tool !== 'ssh-cli' || typeof version.parsed.version !== 'string') {
    errors.push('version payload missing tool/version');
  }
  if (version.status !== 0) errors.push('--json --version exit != 0');
}

const temp = mkdtempSync(join(tmpdir(), 'cli-shell-'));
const validCfg = join(temp, 'ok.conf');
writeFileSync(validCfg, '[dev]\nhost = dev.example.com\nuser = admin\nport = 22\n', 'utf8');
const ok = jsonOf(['--json', 'config', '--check', validCfg]);
if (ok) {
  if (ok.parsed.schema_version !== 1 || ok.parsed.ok !== true || ok.parsed.hosts !== 1) {
    errors.push('config-check ok payload invalid');
  }
  if (ok.status !== 0) errors.push('--json config --check (valid) exit != 0');
}
const bad = jsonOf(['--json', 'config', '--check', join(temp, 'missing.conf')]);
if (bad) {
  if (bad.parsed.schema_version !== 1 || bad.parsed.ok !== false || typeof bad.parsed.error !== 'string') {
    errors.push('config-check error payload invalid');
  }
  if (bad.status !== 3) errors.push('--json config --check (missing) exit != 3');
}
rmSync(temp, { recursive: true, force: true });

// 5. The schema file itself is valid and pins schema_version to 1.
const schema = JSON.parse(readFileSync(join(CLI, 'docs/cli-output.schema.json'), 'utf8'));
if (schema.properties?.schema_version?.const !== 1) errors.push('schema does not pin schema_version to 1');

if (errors.length > 0) {
  console.error(`cli-shell contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('cli-shell contract valid: PowerShell/bash/zsh/fish completion scripts cover the command surface; the man page documents options, commands, and stable exit codes; --json emits versioned schema_version:1 payloads (version, config-check ok/error) with stable exit codes.');