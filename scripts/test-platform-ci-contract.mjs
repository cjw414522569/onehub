#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { PLATFORM_JOBS, validatePlatformCi, validatePlatformCiText } from './validate-platform-ci.mjs';

const root = path.resolve(process.argv[2] ?? process.cwd());
const positiveErrors = validatePlatformCi(root);
if (positiveErrors.length > 0) {
  console.error('Expected the platform CI contract to pass:');
  for (const error of positiveErrors) console.error(`- ${error}`);
  process.exit(1);
}

const workflow = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8');
const branchProtection = fs.readFileSync(path.join(root, '.github/branch-protection.yml'), 'utf8');
const invalidWorkflow = workflow.replace(/\n  platform-android:\n[\s\S]*?(?=\n  [A-Za-z0-9_-]+:\n|$)/, '\n');
const negativeErrors = validatePlatformCiText({ workflow: invalidWorkflow, branchProtection, rootDir: root });
if (!negativeErrors.some((error) => error.includes('platform-android'))) {
  console.error('Negative platform CI fixture did not reject a missing Android job.');
  process.exit(1);
}

console.log(`Platform CI contract tests passed: ${Object.keys(PLATFORM_JOBS).length} platform jobs and missing-job negative fixture.`);
