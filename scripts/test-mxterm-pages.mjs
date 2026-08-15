#!/usr/bin/env node

// T003 contract: mxterm prototype UI pages copied wholesale into
// clients/windows/ui/pages. Verifies all 11 files exist, are non-empty,
// and are byte-identical (sha256) to the mxterm source pages.

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SRC = join(ROOT, 'mxterm/prototype/light-neutral');
const PAGES = join(ROOT, 'clients/windows/ui/pages');
const errors = [];

const sourceFiles = readdirSync(SRC).filter((f) => f.endsWith('.html') || f.endsWith('.md')).sort();
const htmlCount = sourceFiles.filter((f) => f.endsWith('.html')).length;
const mdCount = sourceFiles.filter((f) => f.endsWith('.md')).length;

if (htmlCount !== 9) errors.push(`expected 9 html pages, got ${htmlCount}`);
if (mdCount !== 2) errors.push(`expected 2 design md files, got ${mdCount}`);

for (const file of sourceFiles) {
  const srcPath = join(SRC, file);
  const pagePath = join(PAGES, file);
  if (!existsSync(pagePath)) {
    errors.push(`missing ui/pages/${file}`);
    continue;
  }
  if (statSync(pagePath).size === 0) errors.push(`ui/pages/${file} is empty`);
  const srcHash = createHash('sha256').update(readFileSync(srcPath)).digest('hex');
  const pageHash = createHash('sha256').update(readFileSync(pagePath)).digest('hex');
  if (srcHash !== pageHash) errors.push(`ui/pages/${file} differs from mxterm source`);
}

if (errors.length > 0) {
  console.error(`mxterm-pages contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`mxterm-pages contract valid: ${htmlCount} html pages + ${mdCount} design md files copied byte-identical to ui/pages.`);