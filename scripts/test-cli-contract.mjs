#!/usr/bin/env node

// T143 contract: Rust CLI config / connect / exec / exit-code contract.
// Runs the unit suite (config parsing, exec mapping, interactive vs
// non-interactive output separation) plus real binary E2E exit-code checks
// and an output snapshot (no ANSI in non-interactive stdout).

import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Build + unit tests (config parse, exit-code mapping, output separation).
const check = run('cargo', ['check', '-p', 'cli', '--locked']);
if (check.status !== 0) errors.push(`cargo check -p cli failed:\n${check.stdout}\n${check.stderr}`);
const test = run('cargo', ['test', '-p', 'cli', '--locked']);
if (test.status !== 0) errors.push(`cargo test -p cli failed:\n${test.stdout}\n${test.stderr}`);

// 2. Build the binary and run the E2E exit-code contract.
const build = run('cargo', ['build', '-p', 'cli', '--locked']);
if (build.status !== 0) errors.push(`cargo build -p cli failed:\n${build.stdout}\n${build.stderr}`);
const exe = join(ROOT, 'target/debug/ssh-cli.exe');

const temp = mkdtempSync(join(tmpdir(), 'cli-e2e-'));
const validCfg = join(temp, 'ok.conf');
const invalidCfg = join(temp, 'bad.conf');
writeFileSync(validCfg, '[dev]\nhost = dev.example.com\nuser = admin\nport = 22\n', 'utf8');
writeFileSync(invalidCfg, '[x]\nuser = a\n', 'utf8');

function e2e(args, expectCode, expectStdout) {
  const res = spawnSync(exe, args, { encoding: 'utf8', timeout: 60000 });
  if (res.status !== expectCode) {
    errors.push(`ssh-cli ${args.join(' ')}: expected exit ${expectCode}, got ${res.status}`);
  }
  if (expectStdout !== undefined && !res.stdout.includes(expectStdout)) {
    errors.push(`ssh-cli ${args.join(' ')}: stdout missing '${expectStdout}': ${res.stdout}`);
  }
  return res;
}

e2e(['--version'], 0, 'ssh-cli 0.1.0');
const help = e2e(['--help'], 0, 'usage: ssh-cli');
if (help.stdout.includes('\u001b')) errors.push('--help stdout must not contain ANSI escapes (non-interactive output separation)');
e2e(['badargs'], 2);
e2e(['config', '--check', validCfg], 0, 'config valid: 1 host(s)');
e2e(['config', '--check', invalidCfg], 3);
e2e(['config', '--check', join(temp, 'missing.conf')], 3);
const exec = e2e(['--config', validCfg, 'dev', 'exec', 'ls'], 4);
if (!exec.stderr.includes('connection error')) errors.push('exec must print a stable connection error to stderr');
rmSync(temp, { recursive: true, force: true });

if (errors.length > 0) {
  console.error(`cli-contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('cli-contract valid: config parse, exec mapping, and interactive/non-interactive output separation pass; real binary exit codes are stable (version 0, help 0, usage 2, config 0/3/3, connect 4) with no ANSI in non-interactive stdout.');