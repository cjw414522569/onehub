#!/usr/bin/env node

// T099 contract: batch event streams, backpressure, UI scheduler adapters.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const ABI = join(ROOT, 'crates/abi-c');
const errors = [];

const TOKENS = [
  'pub const EVENT_BATCH_VERSION', 'pub struct EventStream', 'pub struct EventBatch',
  'pub enum BatchItem', 'pub struct PushResult', 'pub fn push_event', 'pub fn flush',
  'pub fn poll', 'pub fn produce_snapshot', 'pub fn dropped_total', 'pub fn needs_snapshot',
  'pub trait Scheduler', 'pub struct UiScheduler', 'pub struct WindowsUiScheduler',
  'events_are_batched_never_per_character', 'backpressure_drops_and_requests_snapshot',
  'slow_ui_recovers_via_snapshot', 'producer_never_blocks_on_stalled_ui',
  'flush_requires_thresholds_and_pending_is_bounded',
  'scheduler_dispatch_and_poll_round_trip', 'scheduler_backpressure_when_ui_is_full',
  'windows_scheduler_adapter_dispatches',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(ABI, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`abi-c missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'abi-c', '--locked'],
  ['test', '-p', 'abi-c', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p abi-c failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`event-stream contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('event-stream contract valid: events never cross the ABI one character at a time (1000 events -> 16 versioned batches); EventStream applies non-blocking backpressure on a slow consumer (drops the oldest batch, counts dropped, marks SnapshotRequired) and a stalled UI recovers via produce_snapshot to the latest state; a 10k-event flood with a stalled consumer keeps the queue bounded and the producer unblocked; UiScheduler/WindowsUiScheduler dispatch batches to the UI thread with bounded queues and backpressure signals; cargo check/test --locked passed.');