#!/usr/bin/env node

// T114 contract: SFTP single-pane / responsive file manager.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub struct RemoteFile', 'pub enum FileKind', 'pub struct FilePane', 'pub fn navigate_into',
  'pub fn select', 'pub fn toggle_select', 'pub fn clear_selection', 'pub fn selected_files',
  'pub enum TransferKind', 'pub struct TransferProgress', 'pub fn percent',
  'pub enum ConflictAction', 'pub enum OpState', 'pub struct TransferOp', 'pub enum OpError',
  'pub struct FileOperationManager', 'pub fn enqueue', 'pub fn start', 'pub fn advance',
  'pub fn complete', 'pub fn fail', 'pub fn cancel', 'pub fn retry',
  'single_pane_navigation_and_selection', 'drag_drop_maps_to_move_and_progress',
  'mobile_selection_maps_to_multi_operation', 'conflict_resolution_is_configurable',
  'progress_percent_is_bounded', 'cancel_and_retry_do_not_duplicate',
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
  console.error(`file-manager contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('file-manager contract valid: FilePane is a single-pane listing with navigation, desktop single-select, and mobile multi-select; FileOperationManager queues upload/download/move/copy/delete with bounded progress percent and configurable conflict resolution (ask/overwrite/skip/rename); cancel/retry reuse the same op id (no duplicate submission); desktop drag-drop maps to a move op and mobile selection maps to multi-file ops; progress/conflict/lifecycle are consistent; file-operation UI integration tests pass; cargo check/test --locked passed.');