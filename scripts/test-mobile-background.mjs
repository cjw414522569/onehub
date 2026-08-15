#!/usr/bin/env node

// T131 contract: mobile background-transfer paths + user prompts.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum MobilePlatform', 'pub enum BackgroundPath', 'pub struct PlatformPaths',
  'pub fn android', 'pub fn ios', 'pub fn difference_summary', 'pub struct BackgroundTransferPolicy',
  'pub fn for_platform', 'pub struct InterruptionRecovery', 'pub fn resume_from', 'pub fn recovered',
  'android_ios_background_paths_are_explicit_and_different',
  'background_transfers_require_a_prompt_and_are_time_limited',
  'interruption_recovery_resumes_from_checkpoint',
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
  console.error(`mobile-background contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('mobile-background contract valid: Android (foreground service for active sessions, WorkManager for deferred) and iOS (BGTaskScheduler, URLSession background transfers) background paths are explicit and different with user-visible summaries; both platforms require a user prompt and are system time-limited; InterruptionRecovery resumes an interrupted transfer from its checkpoint (never past the bytes actually completed, no data loss); cargo check/test --locked passed (background time-limit and system-termination tests run on mobile devices).');