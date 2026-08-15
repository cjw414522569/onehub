#!/usr/bin/env node

// T161 contract: independent security review (SSH / keys / sync / gateway)
// with retest confirmation. Validates the review report (every critical/high
// is fixed or mitigated with retest evidence; every medium has a disposition
// plan), re-runs the security pipeline and the security contracts as the
// retest, and archives the confirmation report. With --write, writes
// docs/reports/SECURITY_REVIEW_T161.json.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 1200000 });
}

// 1. Validate the review report structure and disposition rules.
const review = JSON.parse(readFileSync(join(ROOT, 'security/review/REVIEW_T161.json'), 'utf8'));
if (review.schema_version !== 1) errors.push('review schema_version != 1');
const findings = review.findings ?? [];
if (findings.length === 0) errors.push('review has no findings');
const required = ['id', 'area', 'severity', 'title', 'status', 'disposition', 'retest'];
for (const finding of findings) {
  for (const field of required) {
    if (!finding[field]) errors.push(`finding ${finding.id} missing ${field}`);
  }
  if (['critical', 'high'].includes(finding.severity) && !['fixed', 'mitigated'].includes(finding.status)) {
    errors.push(`finding ${finding.id} (${finding.severity}) is not fixed or mitigated`);
  }
  if (finding.severity === 'medium' && !finding.disposition) {
    errors.push(`medium finding ${finding.id} lacks a disposition plan`);
  }
}
// Summary counts must be consistent.
const high = findings.filter((f) => f.severity === 'high').length;
const critical = findings.filter((f) => f.severity === 'critical').length;
const fixedMitigatedHigh = findings.filter((f) => f.severity === 'high' && ['fixed', 'mitigated'].includes(f.status)).length;
const medium = findings.filter((f) => f.severity === 'medium').length;
const mediumPlanned = findings.filter((f) => f.severity === 'medium' && f.disposition).length;
if (review.summary.critical !== critical) errors.push('summary critical count mismatch');
if (review.summary.high !== high) errors.push('summary high count mismatch');
if (review.summary.high_mitigated_or_fixed !== fixedMitigatedHigh) errors.push('summary high disposition mismatch');
if (review.summary.medium_with_disposition !== mediumPlanned) errors.push('summary medium disposition mismatch');

// 2. Retest: re-run the security pipeline and the security contracts.
const pipeline = run('node', [join(ROOT, 'scripts/test-security-pipeline.mjs'), ROOT]);
if (pipeline.status !== 0) errors.push(`security pipeline retest failed:\n${pipeline.stdout}\n${pipeline.stderr}`);
const contracts = [
  ['test-gateway-auth.mjs', ['node', join(ROOT, 'scripts/test-gateway-auth.mjs'), ROOT]],
  ['test-gateway-address-policy.mjs', ['node', join(ROOT, 'scripts/test-gateway-address-policy.mjs'), ROOT]],
  ['test-gateway-session-protocol.mjs', ['node', join(ROOT, 'scripts/test-gateway-session-protocol.mjs'), ROOT]],
  ['test-gateway-soak.mjs', ['node', join(ROOT, 'scripts/test-gateway-soak.mjs'), ROOT]],
  ['test-telemetry-privacy.mjs', ['node', join(ROOT, 'scripts/test-telemetry-privacy.mjs'), ROOT]],
  ['test-crash-diagnostics.mjs', ['node', join(ROOT, 'scripts/test-crash-diagnostics.mjs'), ROOT]],
];
for (const [name, args] of contracts) {
  const res = run(args[0], args.slice(1));
  if (res.status !== 0) errors.push(`${name} retest failed:\n${res.stdout}\n${res.stderr}`);
}

if (errors.length > 0) {
  console.error(`security-review contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

// 3. Archive the retest confirmation report.
const report = {
  task: 'T161',
  status: 'pass',
  verified_at_utc: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
  review: {
    scope: review.scope,
    summary: review.summary,
    findings: findings.map((f) => ({ id: f.id, area: f.area, severity: f.severity, status: f.status })),
  },
  retest_confirmation: {
    security_pipeline: 'pass',
    security_contracts: contracts.map(([name]) => name),
    note: 'All critical/high findings are fixed or mitigated with retest evidence; every medium finding has a disposition plan.',
  },
};
if (process.argv.includes('--write')) {
  const reportPath = join(ROOT, 'docs/reports/SECURITY_REVIEW_T161.json');
  mkdirSync(dirname(reportPath), { recursive: true });
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  console.log(`wrote ${reportPath}`);
}

console.log(`security-review contract valid: scope ${review.scope.join('/')}; ${critical} critical, ${high} high (${fixedMitigatedHigh} fixed/mitigated), ${medium} medium (${mediumPlanned} with disposition); security pipeline + ${contracts.length} security contracts re-ran green as the retest confirmation.`);