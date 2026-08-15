#!/usr/bin/env node

// T173 contract: 1.0 release readiness review. Validates the checklist
// (product, security, performance, licensing, support, rollback, and
// documentation owners all signed off) and runs the release-readiness
// evidence (RC gate). With --write, archives the validated checklist.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];
const REQUIRED_AREAS = ['product', 'security', 'performance', 'licensing', 'support', 'rollback', 'documentation'];

const checklist = JSON.parse(readFileSync(join(ROOT, 'release/1.0/release-1.0.json'), 'utf8'));
if (checklist.schema_version !== 1) errors.push('release checklist schema_version != 1');
const areas = checklist.checklist ?? [];
const present = areas.map((a) => a.area);
for (const required of REQUIRED_AREAS) {
  if (!present.includes(required)) errors.push(`missing release area: ${required}`);
}
for (const entry of areas) {
  const signOff = entry.sign_off;
  if (!signOff?.name || !signOff?.date || signOff?.status !== 'approved') {
    errors.push(`area ${entry.area} is not signed off`);
  }
  if (!entry.evidence) errors.push(`area ${entry.area} missing evidence reference`);
}

// Release-readiness evidence: the RC gate must pass.
const rc = spawnSync('node', [join(ROOT, 'scripts/test-rc-gate.mjs'), ROOT], { cwd: ROOT, encoding: 'utf8', timeout: 1800000 });
if (rc.status !== 0) errors.push(`RC gate failed:\n${rc.stdout?.slice(0, 2000)}\n${rc.stderr?.slice(0, 2000)}`);

if (errors.length > 0) {
  console.error(`release-1.0 contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

if (process.argv.includes('--write')) {
  const report = {
    task: 'T173', status: 'pass',
    verified_at_utc: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    release: '1.0.0',
    owners: REQUIRED_AREAS.map((area) => ({ area, sign_off: 'approved' })),
    rc_gate: 'pass',
  };
  const reportPath = join(ROOT, 'release/1.0/release-1.0.checklist.json');
  mkdirSync(dirname(reportPath), { recursive: true });
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  console.log(`wrote ${reportPath}`);
}

console.log(`release-1.0 contract valid: all 7 areas (product, security, performance, licensing, support, rollback, documentation) are signed off with evidence; the RC gate passes; release 1.0.0 is ready for release.`);