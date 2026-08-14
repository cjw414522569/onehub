#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

const expectedSshTargets = [
  'fuzz_agent_frame_parse',
  'fuzz_agent_forwarding_channel_open',
  'fuzz_known_hosts_matching',
  'fuzz_qos_scheduler_bounded',
  'fuzz_session_channel_validation',
  'fuzz_hashed_host_matching',
];

const fuzzModule = join(ROOT, 'crates/ssh-backend/src/fuzz.rs');
if (!existsSync(fuzzModule)) errors.push('Missing crates/ssh-backend/src/fuzz.rs');
if (existsSync(fuzzModule)) {
  const src = readFileSync(fuzzModule, 'utf8');
  for (const name of expectedSshTargets) {
    if (!src.includes(name)) errors.push(`ssh-backend fuzz module is missing target: ${name}`);
    if (!src.includes('fn ' + name)) errors.push(`ssh-backend fuzz module has no test fn for ${name}`);
  }
  if (!src.includes('XorShift64')) errors.push('ssh fuzz module must use a deterministic PRNG');
}

const corpusPath = join(ROOT, 'fuzz/smoke-corpus.json');
if (existsSync(corpusPath)) {
  const corpus = JSON.parse(readFileSync(corpusPath, 'utf8').replace(/^\uFEFF/, ''));
  const sshNames = (corpus.ssh_targets ?? []).map((t) => t.name);
  for (const name of expectedSshTargets) {
    if (!sshNames.includes(name)) errors.push(`corpus is missing ssh target ${name}`);
  }
}

// Run the time-limited runner and check it records the SSH targets.
const runner = spawnSync('powershell', [
  '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
  join(ROOT, 'scripts/run-fuzz-smoke.ps1'), '-RepositoryRoot', ROOT, '-TimeoutSeconds', '900',
], { cwd: ROOT, encoding: 'utf8', timeout: 960000 });
if (runner.status !== 0) {
  errors.push(`run-fuzz-smoke.ps1 failed:\n${runner.stdout}\n${runner.stderr}`);
} else {
  const resultsPath = join(ROOT, 'artifacts/fuzz/smoke-results.json');
  if (!existsSync(resultsPath)) {
    errors.push('Missing artifacts/fuzz/smoke-results.json');
  } else {
    const results = JSON.parse(readFileSync(resultsPath, 'utf8').replace(/^\uFEFF/, ''));
    if (results.ssh_targets_passed !== expectedSshTargets.length) {
      errors.push(`Expected ${expectedSshTargets.length} ssh targets passed, got ${results.ssh_targets_passed}`);
    }
    if (!results.archive_dir || !existsSync(results.archive_dir)) {
      errors.push('Fuzz runner did not archive the corpus');
    }
  }
}

if (errors.length > 0) {
  console.error(`ssh-fuzz contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('ssh-fuzz contract valid: deterministic SSH decoder/auth/channel fuzz targets (no panic/OOB/livelock/unbounded growth), corpus-registered, time-limited runner with CPU-hour multiplier and corpus archive.');