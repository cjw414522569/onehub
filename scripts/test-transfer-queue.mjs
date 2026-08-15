#!/usr/bin/env node

// T115 contract: transfer queue, background progress, failure retry, safe notifications.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum QueueState', 'pub enum FailureKind', 'pub struct Failure', 'pub enum RetryPolicy',
  'pub struct QueueProgress', 'pub struct QueueEntry', 'pub struct TransferNotification',
  'pub fn contains', 'pub struct QueueStats', 'pub struct TransferQueue', 'pub enum QueueError',
  'pub fn enqueue', 'pub fn start', 'pub fn advance', 'pub fn complete', 'pub fn fail',
  'pub fn retry', 'pub fn cancel', 'pub fn stats', 'pub fn notification_for',
  'queue_lifecycle_and_stats', 'cancel_retry_do_not_duplicate',
  'transient_failures_auto_retry_under_policy', 'permanent_failure_does_not_auto_retry',
  'notifications_do_not_leak_secrets', 'background_progress_advances',
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
  console.error(`transfer-queue contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('transfer-queue contract valid: TransferQueue manages transfers with background progress, transient/permanent failure classification (transient auto-retries under the RetryPolicy, permanent never does), and manual retry/cancel that reuse the same entry id (no duplicate submission); notification_for builds system notifications from the safe label only (never source/destination paths), verified by the notification-leak test; queue failure and lifecycle tests pass; cargo check/test --locked passed.');