#!/usr/bin/env node

// T144 contract: CLI forwarding / SFTP / proxy-chain capabilities sharing
// the GUI core. Runs the unit suite (CLI/GUI core agreement: config
// equality, byte-identical proxy wire, shared streaming engine) plus real
// binary `cap` command E2E.

import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Build + unit tests (CLI/GUI core agreement).
const check = run('cargo', ['check', '-p', 'cli', '--locked']);
if (check.status !== 0) errors.push(`cargo check -p cli failed:\n${check.stdout}\n${check.stderr}`);
const test = run('cargo', ['test', '-p', 'cli', '--locked']);
if (test.status !== 0) errors.push(`cargo test -p cli failed:\n${test.stdout}\n${test.stderr}`);

// 2. Build the binary and run the `cap` command E2E.
const build = run('cargo', ['build', '-p', 'cli', '--locked']);
if (build.status !== 0) errors.push(`cargo build -p cli failed:\n${build.stdout}\n${build.stderr}`);
const exe = join(ROOT, 'target/debug/ssh-cli.exe');

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

e2e(['cap', 'forward', '--listen', '127.0.0.1:1337', '--target', 'db.internal:5432'], 0,
  'forward spec: 127.0.0.1:1337 -> db.internal:5432 scope=Loopback');
e2e(['cap', 'sftp'], 0, 'sftp spec: chunk=65536 in_flight=8');
e2e(['cap', 'proxy', '--chain', 'socks5://proxy.example:1080', '--target', '93.184.216.34', '--port', '22'], 0,
  'proxy chain first-hop: 05 01 00 05 01 00 01 5d b8 d8 22 00 16');
// An HTTP-CONNECT first hop is rejected (first hop must be SOCKS5).
e2e(['cap', 'proxy', '--chain', 'http://proxy.example:3128', '--target', 'db.internal', '--port', '22'], 3);

if (errors.length > 0) {
  console.error(`cli-capabilities contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('cli-capabilities contract valid: CLI forwarding/SFTP/proxy-chain commands build the exact shared-core configs the GUI uses (LocalForwardConfig, StreamConfig, SOCKS5 wire bytes) with no behavior divergence; CLI/GUI core-agreement unit tests and real binary cap E2E pass.');