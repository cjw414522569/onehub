#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/core-protocol');
const errors = [];

const FORBIDDEN_DEPENDENCIES = [
  'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui', 'windows-app-sdk',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'tokio', 'wgpu', 'harfbuzz',
];
const REQUIRED_TOKENS = [
  'pub enum Capability', 'pub struct CapabilitySet', 'pub struct NegotiationResult',
  'pub fn negotiate', 'pub enum PlatformId', 'pub struct PlatformProfile',
  'pub fn negotiate_with_platform', 'ALL_CAPABILITIES',
  'intersection_property_holds_for_all_subset_combinations',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/core-protocol/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
const dependencyNames = [];
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  dependencyNames.push(name);
  if (/\{\s*path\s*=/.test(trimmed) && !['core-domain', 'core-errors'].includes(name)) {
    errors.push(`core-protocol must only path-depend on core crates (found ${name})`);
  }
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`core-protocol has forbidden dependency: ${name}`);
}
for (const name of dependencyNames) {
  if (!['core-domain', 'core-errors', 'serde'].includes(name)) {
    errors.push(`core-protocol runtime dependency is not approved: ${name}`);
  }
}

const sourceFiles = [];
function collect(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collect(absolute);
    else if (entry.name.endsWith('.rs')) sourceFiles.push(absolute);
  }
}
if (existsSync(join(CRATE, 'src'))) collect(join(CRATE, 'src'));
const sourceText = sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`core-protocol is missing required token: ${token}`);
}

// Every schema feature id must be covered by the Capability enum's as_str.
const schema = JSON.parse(readFileSync(join(ROOT, 'protocol/schema/domain-v1.json'), 'utf8'));
const schemaFeatureIds = schema.capability_negotiation?.feature_ids ?? [];
for (const featureId of schemaFeatureIds) {
  if (!sourceText.includes(`"${featureId}"`)) {
    errors.push(`Capability must cover schema feature id: ${featureId}`);
  }
}
if (schemaFeatureIds.length !== 7) errors.push(`Expected 7 schema feature ids, found ${schemaFeatureIds.length}`);

for (const args of [['check', '-p', 'core-protocol', '--locked'], ['test', '-p', 'core-protocol', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p core-protocol failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`capabilities contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`capabilities contract valid: ${schemaFeatureIds.length} schema feature ids covered, negotiate algebra + platform profiles, cargo check/test --locked passed.`);