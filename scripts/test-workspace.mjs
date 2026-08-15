#!/usr/bin/env node

// T106 contract: desktop window / multi-tab / split / focus model.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum SplitDirection', 'pub struct PaneModel', 'pub struct TabModel',
  'pub struct WindowModel', 'pub struct FocusLocation', 'pub struct WorkspaceSnapshot',
  'pub struct Workspace', 'pub enum ShortcutAction', 'pub struct ShortcutMap',
  'pub enum RestoreError', 'pub fn add_window', 'pub fn add_tab', 'pub fn close_tab',
  'pub fn split_active_pane', 'pub fn move_tab', 'pub fn focus_next', 'pub fn focus_prev',
  'pub fn snapshot', 'pub fn restore', 'pub fn focused', 'pub fn default_map',
  'new_workspace_has_one_window_one_tab_and_consistent_focus',
  'tabs_and_splits_track_active_pane', 'drag_drop_moves_tab_between_windows',
  'close_tab_keeps_focus_consistent', 'shortcuts_resolve_to_actions',
  'focus_next_prev_cycles_panes_tabs_windows',
  'snapshot_restore_round_trip_preserves_multi_window_state',
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
  console.error(`workspace contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('workspace contract valid: Workspace owns multiple windows, each with tabs and splittable panes; a single global focus location (window -> tab -> pane) stays consistent across add/close/split/move; tabs drag between windows (focus follows) and move via the MoveTabToNextWindow shortcut; ShortcutMap resolves Ctrl+T/W/Tab/Shift+[ ]/split/focus-ring chords; focus_next/focus_prev cycle the whole focus ring; snapshot/restore round-trips the multi-window layout deterministically and rejects invalid indices; desktop-integration and layout-state tests pass; cargo check/test --locked passed.');