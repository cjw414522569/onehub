#!/usr/bin/env node

// T100 token lint: validates the design-system token schema, resolves every
// semantic/high-contrast color reference, checks breakpoint ordering, and
// enforces WCAG contrast on the declared pairs for both themes.

import { resolve } from 'node:path';
import { loadTokens, lint } from './lib/design-tokens.mjs';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const tokens = loadTokens(ROOT);
const errors = lint(tokens);
if (errors.length > 0) {
  console.error(`design-token lint failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('design-token lint valid: required families present, semantic/high-contrast color references resolve, breakpoints strictly increasing, and every declared contrast pair (baseline + high-contrast) meets the minimum WCAG ratio.');