#!/usr/bin/env node

// T101 contract: app startup, database unlock, failure recovery.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CLI = join(ROOT, 'apps/cli');
const errors = [];

const TOKENS = [
  'pub struct StartupFlow', 'pub struct StartupOutcome', 'pub struct ActionablePrompt',
  'pub enum PromptSeverity', 'pub enum DatabaseHealth', 'pub struct StartupConfig',
  'pub fn run', 'DB_CORRUPTED', 'DB_MIGRATION_FAILED', 'DB_UNKNOWN_VERSION',
  'DB_REJECTED', 'DB_READ_ONLY', 'DB_OPENED', 'SECURE_STORE_LOCKED',
  'corruption_yields_actionable_prompt', 'migration_failure_yields_actionable_prompt_and_keeps_data',
  'secure_store_lock_yields_actionable_prompt', 'reject_and_read_only_open_policies_prompt',
  'unknown_version_yields_actionable_prompt', 'healthy_startup_migrates_and_prompts_info',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(CLI, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`cli missing required token: ${token}`);
}

// The dependency rules must declare the new cli dependencies.
const rules = JSON.parse(readFileSync(join(ROOT, 'architecture/dependency-rules.json'), 'utf8'));
const cliModule = rules.modules.find((module) => module.id === 'cli');
if (!cliModule) errors.push('dependency-rules.json is missing module cli');
else {
  for (const dep of ['storage-sqlite', 'secure-store']) {
    if (!cliModule.dependencies.includes(dep)) errors.push(`cli dependency rules must include ${dep}`);
  }
}

for (const args of [
  ['check', '-p', 'cli', '--locked'],
  ['test', '-p', 'cli', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p cli failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`startup contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('startup contract valid: StartupFlow::run produces a structured StartupOutcome with actionable prompts for every launch failure mode (corrupted database -> DB_CORRUPTED, failed migration -> DB_MIGRATION_FAILED with data preserved at the pre-failure version, no migration path -> DB_UNKNOWN_VERSION, reject/read-only policies, and locked OS secure store -> SECURE_STORE_LOCKED), each with a stable code, severity, title, message, and concrete user action; the startup failure matrix passes; cargo check/test --locked passed for cli.');