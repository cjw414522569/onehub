#!/usr/bin/env node

// T118 i18n resource lint: shared message IDs, plurals, dates, RTL basics,
// and truncation checks (real + pseudo-localized lengths).

import { resolve } from 'node:path';
import { lint } from './lib/i18n.mjs';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = lint(ROOT);
if (errors.length > 0) {
  console.error(`i18n lint failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('i18n lint valid: en and zh-CN share identical message IDs with matching placeholders, plural pairs are complete, date/time formats are present, RTL layout basics are declared, and no message exceeds the truncation limit (real or pseudo-localized).');