#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { validateWorkspace } from './validate-workspace.mjs';

const root = path.resolve(process.argv[2] ?? process.cwd());
const positiveErrors = validateWorkspace(root);
if (positiveErrors.length > 0) {
  console.error('Expected the current workspace contract to pass, but it failed:');
  for (const error of positiveErrors) console.error(`- ${error}`);
  process.exit(1);
}

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'ssh-workspace-contract-'));
try {
  fs.mkdirSync(path.join(fixture, 'architecture'), { recursive: true });
  fs.copyFileSync(path.join(root, 'architecture/dependency-rules.json'), path.join(fixture, 'architecture/dependency-rules.json'));
  const negativeErrors = validateWorkspace(fixture);
  if (!negativeErrors.some((error) => error.includes('Missing root Cargo.toml workspace manifest.'))) {
    console.error('Negative fixture did not reject a missing root workspace manifest.');
    process.exit(1);
  }
} finally {
  fs.rmSync(fixture, { recursive: true, force: true });
}

console.log('Workspace contract tests passed: positive workspace and negative missing-manifest fixture.');
