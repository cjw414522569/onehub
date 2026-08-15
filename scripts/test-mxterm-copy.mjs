#!/usr/bin/env node

// T002 contract: mxterm frontend copied wholesale into clients/windows/ui.
// Verifies the copy matches the mxterm source (file count + sha256 per file)
// and that the MIT license attribution is present.

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SRC = join(ROOT, 'mxterm');
const UI = join(ROOT, 'clients/windows/ui');
const errors = [];

function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function collect(dir, base, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const abs = join(dir, entry.name);
    if (entry.isDirectory()) collect(abs, base, files);
    else files.push(relative(base, abs).replaceAll('\\', '/'));
  }
}

// 1) Root files expected to be copied byte-identical.
const ROOT_FILES = [
  'index.html',
  'package.json',
  'package-lock.json',
  'vite.config.ts',
  'tsconfig.json',
  'tsconfig.node.json',
];
for (const file of ROOT_FILES) {
  const uiPath = join(UI, file);
  const srcPath = join(SRC, file);
  if (!existsSync(uiPath)) {
    errors.push(`missing ui/${file}`);
  } else if (sha256(srcPath) !== sha256(uiPath)) {
    errors.push(`ui/${file} differs from mxterm source`);
  }
}

// 2) src/ tree identical: same file count and per-file sha256.
const srcFiles = [];
collect(join(SRC, 'src'), join(SRC, 'src'), srcFiles);
const uiSrcFiles = [];
collect(join(UI, 'src'), join(UI, 'src'), uiSrcFiles);
if (srcFiles.length !== uiSrcFiles.length) {
  errors.push(`src file count mismatch: mxterm=${srcFiles.length} ui=${uiSrcFiles.length}`);
}
for (const rel of srcFiles) {
  const uiPath = join(UI, 'src', rel);
  if (!existsSync(uiPath)) {
    errors.push(`ui/src/${rel} missing`);
    continue;
  }
  if (sha256(join(SRC, 'src', rel)) !== sha256(uiPath)) {
    errors.push(`ui/src/${rel} differs from mxterm source`);
  }
}

// 3) MIT license attribution.
if (!existsSync(join(UI, 'LICENSE.mxterm'))) {
  errors.push('missing ui/LICENSE.mxterm');
} else {
  const license = readFileSync(join(UI, 'LICENSE.mxterm'), 'utf8');
  if (!license.includes('MIT License')) errors.push('LICENSE.mxterm must carry the MIT license text');
}
if (!existsSync(join(UI, 'README.md'))) {
  errors.push('missing ui/README.md');
} else {
  const readme = readFileSync(join(UI, 'README.md'), 'utf8');
  if (!readme.includes('mxterm') || !readme.includes('MIT')) {
    errors.push('ui/README.md must carry mxterm attribution and MIT notice');
  }
}

if (errors.length > 0) {
  console.error(`mxterm-copy contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(
  `mxterm-copy contract valid: ${uiSrcFiles.length} src files + ${ROOT_FILES.length} root files copied byte-identical from mxterm; MIT attribution present.`
);