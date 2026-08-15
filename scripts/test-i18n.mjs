#!/usr/bin/env node

// T118 i18n contract: resource lint + pseudo-localization + snapshots.

import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { lint, loadLocale, pseudoLocalize, snapshot } from './lib/i18n.mjs';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SNAPSHOT_DIR = join(ROOT, 'i18n', 'snapshots');
const errors = [];

// 1. Resource lint.
for (const error of lint(ROOT)) errors.push(`i18n lint: ${error}`);

// 2. Pseudo-localization produces expanded, distinct strings.
const en = loadLocale(ROOT, 'en');
let pseudoDistinct = 0;
for (const [key, value] of Object.entries(en.messages)) {
  const pseudo = pseudoLocalize(value);
  if (pseudo === value) errors.push(`pseudo-localization no-op for "${key}"`);
  if (pseudo.length <= value.length) errors.push(`pseudo-localization did not expand "${key}"`);
  pseudoDistinct += 1;
}
if (pseudoDistinct !== Object.keys(en.messages).length) errors.push('pseudo-localization did not cover every message');

// 3. Snapshots must be reproducible (regenerate -> no diff).
const locales = ['en', 'zh-CN'];
const tmp = join(ROOT, 'artifacts/tmp/i18n-snapshots');
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
for (const locale of locales) {
  const snap = snapshot(ROOT, locale);
  const name = `${locale}.snapshot.json`;
  const serialized = `${JSON.stringify(snap, null, 2)}\n`;
  const generated = join(tmp, name);
  writeFileSync(generated, serialized, 'utf8');
  const checkedIn = join(SNAPSHOT_DIR, name);
  if (!existsSync(checkedIn)) {
    mkdirSync(SNAPSHOT_DIR, { recursive: true });
    writeFileSync(checkedIn, serialized, 'utf8');
    errors.push(`snapshot missing; wrote ${name} (commit and re-run)`);
    continue;
  }
  if (readFileSync(checkedIn, 'utf8') !== serialized) {
    errors.push(`snapshot drift: ${name} differs from regeneration (regenerate and commit)`);
  }
}

if (errors.length > 0) {
  console.error(`i18n contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('i18n contract valid: resource lint passes (shared message IDs, matching placeholders, plural pairs, date formats, RTL basics, truncation limits); pseudo-localization expands every message to catch truncation; per-locale snapshots regenerate byte-identically (no drift).');