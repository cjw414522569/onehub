#!/usr/bin/env node

// T102 contract: host library (list/group/tags/search/sort + 10k perf).

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub struct HostRecord', 'pub struct HostLibrary', 'pub enum SortField', 'pub enum SortOrder',
  'pub struct GroupSummary', 'pub struct TagSummary', 'pub struct SelectionModel',
  'pub fn search', 'pub fn filter_by_tag', 'pub fn groups', 'pub fn tags', 'pub fn sorted',
  'pub fn view', 'pub fn matches_query',
  'search_matches_name_host_tag_and_group_case_insensitively', 'tag_filter_and_summaries',
  'sort_by_name_group_and_recency', 'view_golden_is_deterministic', 'remove_updates_library',
  'ten_thousand_hosts_search_filter_sort_under_budget',
  'selection_model_navigates_with_keyboard_and_touch',
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

const rules = JSON.parse(readFileSync(join(ROOT, 'architecture/dependency-rules.json'), 'utf8'));
const module = rules.modules.find((item) => item.id === 'host-library');
if (!module) errors.push('dependency-rules.json is missing module host-library');
else if (module.layer !== 'L1' || module.path !== 'crates/host-library') {
  errors.push('host-library rules must be L1 at crates/host-library');
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
  console.error(`host-library contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('host-library contract valid: HostLibrary lists/inserts/removes HostRecords with case-insensitive search across name/host/tags/group, tag filtering, group and tag summaries, and deterministic sorting (name/host/group/last-used, asc/desc); SelectionModel provides stable index-based keyboard/touch navigation with clamping; the deterministic view golden passes; the 10k-host performance test keeps search/filter/sort well under an interactive budget; cargo check/test --locked passed.');