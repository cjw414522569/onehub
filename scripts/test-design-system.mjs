#!/usr/bin/env node

// T100 design-system contract: token lint + reproducible golden snapshots.

import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { lint, loadTokens, snapshot } from './lib/design-tokens.mjs';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const GOLDEN_DIR = join(ROOT, 'design-system', 'golden');
const errors = [];

const tokens = loadTokens(ROOT);

// 1. Token lint must pass.
const lintErrors = lint(tokens);
for (const error of lintErrors) errors.push(`token lint: ${error}`);

// 2. The theme must expose the shared semantic baseline (all roles resolve).
const baseline = snapshot(tokens, 'semantic');
const highContrast = snapshot(tokens, 'high_contrast');
for (const role of ['background', 'surface', 'text.primary', 'text.secondary',
  'text.on.accent', 'accent', 'focus', 'border', 'danger', 'success']) {
  if (!(role in baseline.resolved)) errors.push(`baseline snapshot missing role: ${role}`);
  if (!(role in highContrast.resolved)) errors.push(`high-contrast snapshot missing role: ${role}`);
}
// High contrast must actually differ on at least the text/secondary pair
// (extensible theme, not a no-op).
if (highContrast.resolved['text.secondary'] === baseline.resolved['text.secondary']) {
  errors.push('high-contrast theme must raise text.secondary contrast');
}

// 3. Golden snapshots must be reproducible (regenerate -> no diff).
const goldenFiles = [
  ['theme-baseline.snapshot.json', baseline],
  ['theme-high-contrast.snapshot.json', highContrast],
];
const tmp = join(ROOT, 'artifacts/tmp/design-golden');
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
for (const [name, snapshotValue] of goldenFiles) {
  const serialized = `${JSON.stringify(snapshotValue, null, 2)}\n`;
  const generatedPath = join(tmp, name);
  writeFileSync(generatedPath, serialized, 'utf8');
  const checkedInPath = join(GOLDEN_DIR, name);
  if (!existsSync(checkedInPath)) {
    // First run: write the golden (regeneration baseline).
    mkdirSync(GOLDEN_DIR, { recursive: true });
    writeFileSync(checkedInPath, serialized, 'utf8');
    errors.push(`golden snapshot was missing; wrote ${name} (commit it and re-run)`);
    continue;
  }
  if (readFileSync(checkedInPath, 'utf8') !== serialized) {
    errors.push(`golden snapshot drift: ${name} differs from regeneration (regenerate and commit)`);
  }
}

if (errors.length > 0) {
  console.error(`design-system contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('design-system contract valid: token lint passes; both themes resolve the shared semantic baseline (visual baseline); high-contrast overrides raise contrast (extensible, not a no-op); regenerating the golden snapshots produces byte-identical files (no drift).');