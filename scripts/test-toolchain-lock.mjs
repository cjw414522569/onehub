#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { validateToolchainLock, validateToolchainModel } from './validate-toolchain-lock.mjs';

const root = path.resolve(process.argv[2] ?? process.cwd());
const positive = validateToolchainLock(root);
if (positive.errors.length > 0) {
  console.error('Expected the current toolchain lock to pass:');
  for (const error of positive.errors) console.error(`- ${error}`);
  process.exit(1);
}

const model = JSON.parse(fs.readFileSync(path.join(root, 'toolchains/toolchain.lock.json'), 'utf8'));
const invalid = structuredClone(model);
invalid.node.node = 'latest';
const negative = validateToolchainModel(invalid);
if (!negative.some((error) => error.includes('node.node'))) {
  console.error('Negative toolchain fixture did not reject a floating Node version.');
  process.exit(1);
}

const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'ssh-toolchain-lock-'));
try {
  fs.mkdirSync(path.join(temp, 'toolchains'), { recursive: true });
  fs.copyFileSync(path.join(root, 'toolchains/toolchain.lock.json'), path.join(temp, 'toolchains/toolchain.lock.json'));
  const missingFiles = validateToolchainLock(temp, { probe: false });
  if (!missingFiles.errors.some((error) => error.includes('Missing .nvmrc.'))) {
    console.error('Negative missing-file fixture did not reject the missing .nvmrc.');
    process.exit(1);
  }
} finally {
  fs.rmSync(temp, { recursive: true, force: true });
}

console.log('Toolchain lock tests passed: positive lock/probe and negative floating-version/missing-file fixtures.');
