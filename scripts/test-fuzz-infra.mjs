#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

const corpusPath = join(ROOT, 'fuzz/smoke-corpus.json');
if (!existsSync(corpusPath)) errors.push('Missing fuzz/smoke-corpus.json');
let corpus = null;
if (existsSync(corpusPath)) {
  corpus = JSON.parse(readFileSync(corpusPath, 'utf8').replace(/^\uFEFF/, ''));
  if (corpus.schema_version !== 1) errors.push('smoke-corpus.json must declare schema_version=1');
  if (!Array.isArray(corpus.targets) || corpus.targets.length === 0) errors.push('smoke-corpus.json must declare targets');
  if (!Array.isArray(corpus.ssh_targets) || corpus.ssh_targets.length === 0) errors.push('smoke-corpus.json must declare ssh_targets');
}

for (const relative of [
  'crates/fuzz-targets/Cargo.toml',
  'crates/fuzz-targets/src/lib.rs',
  'crates/fuzz-targets/README.md',
  'scripts/run-fuzz-smoke.ps1',
]) {
  if (!existsSync(join(ROOT, relative))) errors.push(`Missing ${relative}`);
}

const expectedTargets = [
  'fuzz_known_hosts_parse',
  'fuzz_terminal_snapshot_deserialize',
  'fuzz_proxy_chain_validation',
  'fuzz_session_state_machine',
  'fuzz_forwarding_table',
  'fuzz_settings_migration',
  'fuzz_command_resolution',
];
const expectedSshTargets = [
  'fuzz_agent_frame_parse',
  'fuzz_agent_forwarding_channel_open',
  'fuzz_known_hosts_matching',
  'fuzz_qos_scheduler_bounded',
  'fuzz_session_channel_validation',
  'fuzz_hashed_host_matching',
];
if (corpus) {
  const names = corpus.targets.map((t) => t.name);
  for (const name of expectedTargets) {
    if (!names.includes(name)) errors.push(`Corpus is missing target: ${name}`);
  }
  if (corpus.targets.length !== expectedTargets.length) {
    errors.push(`Expected ${expectedTargets.length} core targets, found ${corpus.targets.length}`);
  }
  const sshNames = corpus.ssh_targets.map((t) => t.name);
  for (const name of expectedSshTargets) {
    if (!sshNames.includes(name)) errors.push(`Corpus is missing ssh target: ${name}`);
  }
  if (corpus.ssh_targets.length !== expectedSshTargets.length) {
    errors.push(`Expected ${expectedSshTargets.length} ssh targets, found ${corpus.ssh_targets.length}`);
  }
}

// The ssh-backend fuzz module must exist and register all ssh targets.
const sshLib = join(ROOT, 'crates/ssh-backend/src/fuzz.rs');
if (!existsSync(sshLib)) errors.push('Missing crates/ssh-backend/src/fuzz.rs');
if (existsSync(sshLib)) {
  const src = readFileSync(sshLib, 'utf8');
  for (const name of expectedSshTargets) {
    if (!src.includes(name)) errors.push(`ssh-backend fuzz module is missing target: ${name}`);
  }
}

// Run the time-limited runner and check its recorded results.
const runner = spawnSync('powershell', [
  '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
  join(ROOT, 'scripts/run-fuzz-smoke.ps1'), '-RepositoryRoot', ROOT, '-TimeoutSeconds', '900',
], { cwd: ROOT, encoding: 'utf8', timeout: 960000 });
if (runner.status !== 0) {
  errors.push(`run-fuzz-smoke.ps1 failed:\n${runner.stdout}\n${runner.stderr}`);
} else {
  const resultsPath = join(ROOT, 'artifacts/fuzz/smoke-results.json');
  if (!existsSync(resultsPath)) {
    errors.push('Missing artifacts/fuzz/smoke-results.json after run');
  } else {
    const results = JSON.parse(readFileSync(resultsPath, 'utf8').replace(/^\uFEFF/, ''));
    if (results.status !== 'pass') errors.push('Fuzz smoke results are not pass');
    if (results.core_targets_passed !== expectedTargets.length) {
      errors.push(`Expected ${expectedTargets.length} core targets passed, got ${results.core_targets_passed}`);
    }
    if (results.ssh_targets_passed !== expectedSshTargets.length) {
      errors.push(`Expected ${expectedSshTargets.length} ssh targets passed, got ${results.ssh_targets_passed}`);
    }
    if (!results.archive_dir || !existsSync(results.archive_dir)) {
      errors.push('Fuzz smoke did not archive the corpus');
    }
  }
}

if (errors.length > 0) {
  console.error(`fuzz infrastructure contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`fuzz infrastructure contract valid: ${expectedTargets.length} core + ${expectedSshTargets.length} ssh corpus targets, time-limited runner, persisted results, corpus archived.`);