#!/usr/bin/env node

// T126 Linux packaging policy lint: formats, dependencies, sandbox, updates.

import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const path = join(ROOT, 'packaging', 'linux', 'policy.json');
const policy = JSON.parse(readFileSync(path, 'utf8'));
const errors = [];

if (policy.schema_version !== 1) errors.push('schema_version must be 1');
for (const format of ['deb', 'rpm', 'appimage', 'flatpak']) {
  const entry = policy.formats?.[format];
  if (!entry) {
    errors.push(`missing format: ${format}`);
    continue;
  }
  if (!entry.targets?.length) errors.push(`${format}: missing targets`);
  if (!entry.dependencies?.length) errors.push(`${format}: missing dependencies`);
  if (typeof entry.sandbox !== 'string' || !entry.sandbox) errors.push(`${format}: missing sandbox`);
  if (!(entry.auto_update in (policy.auto_update_boundaries ?? {}))) {
    errors.push(`${format}: auto_update must be a declared boundary`);
  }
}
const flatpak = policy.formats?.flatpak;
if (flatpak?.sandbox !== 'sandboxed') errors.push('flatpak must be sandboxed');
if (!flatpak?.permissions?.length) errors.push('flatpak must declare minimal permissions');
if (!policy.reproducibility?.clean_ci_builds
  || !policy.reproducibility?.source_timestamps
  || !policy.reproducibility?.deterministic_archives) {
  errors.push('reproducibility must be enabled (clean CI, source timestamps, deterministic archives)');
}

if (errors.length > 0) {
  console.error(`linux-packaging lint failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('linux-packaging lint valid: deb/rpm/AppImage/Flatpak all declare targets, dependencies, sandbox, and an auto-update boundary inside the declared set; Flatpak is sandboxed with minimal permissions; reproducibility is enabled.');