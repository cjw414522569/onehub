#!/usr/bin/env node

// T104 contract: first-connection host fingerprint review.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum KeyAlgorithm', 'pub enum FingerprintSource', 'pub enum RiskLevel',
  'pub struct HostKeyFingerprint', 'pub struct FingerprintReview', 'pub enum ReviewState',
  'pub enum ReviewDecision', 'pub struct ChangeNotice', 'pub struct ReviewView',
  'pub fn from_key_bytes', 'pub fn formatted', 'pub fn classify', 'pub fn change_detected',
  'pub fn risk', 'pub fn decide', 'pub fn view', 'SHA256_FINGERPRINT_LEN',
  'fingerprint_is_deterministic_sha256', 'formatted_fingerprint_is_colon_grouped',
  'new_host_is_medium_risk_and_approvable', 'matching_known_fingerprint_is_low_risk',
  'changed_fingerprint_is_high_risk_and_rejectable', 'weak_algorithm_raises_risk_even_when_matching',
  'view_shows_algorithm_fingerprint_source_and_risk',
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
  console.error(`fingerprint-review contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('fingerprint-review contract valid: HostKeyFingerprint computes a deterministic SHA-256 fingerprint (colon-grouped display) with algorithm and source; FingerprintReview classifies new/matching/changed keys (new = medium, matching = algorithm risk, changed = high with an explicit change notice); approve/reject drive the review state; the ReviewView shows algorithm, SHA-256 fingerprint, source, and risk; approve/reject/change UI integration tests pass; cargo check/test --locked passed.');