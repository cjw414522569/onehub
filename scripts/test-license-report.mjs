import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const reportPath = 'artifacts/reports/LICENSE_COMPLIANCE.json';
const policyPath = 'architecture/LICENSE_POLICY.md';
assert.equal(existsSync(reportPath), true, 'license report must exist');
assert.equal(existsSync(policyPath), true, 'license policy must exist');

const report = JSON.parse(readFileSync(reportPath, 'utf8').replace(/^\uFEFF/, ''));
const policy = readFileSync(policyPath, 'utf8');

assert.equal(report.schema_version, 1);
assert.equal(report.project_license, 'AGPL-3.0');
assert.deepEqual(report.scopes, ['release_candidate', 'development_only']);
assert.ok(Array.isArray(report.dependencies) && report.dependencies.length > 0);
assert.equal(report.metadata_inputs.length, 4);
assert.ok(report.metadata_inputs.every((input) => input.exists === true && /^[0-9a-f]{64}$/i.test(String(input.sha256))));
assert.ok(Array.isArray(report.restrictions));
assert.ok(Array.isArray(report.release_blockers));
assert.equal(report.release_blockers.length, 0);
assert.ok(['pass', 'pass_with_restrictions'].includes(report.status));
assert.ok(report.summary.release_dependency_count > 0);
assert.ok(report.summary.development_restriction_count >= 1);
assert.equal(report.distribution.project_license_file.exists, true);
assert.equal(report.distribution.notice_file.exists, true);
assert.equal(report.distribution.project_license_file.sha256.length, 64);
assert.equal(report.distribution.notice_file.sha256.length, 64);
assert.equal(report.distribution.linkage_policy.static.length > 0, true);
assert.equal(report.distribution.linkage_policy.dynamic.length > 0, true);
assert.equal(report.distribution.linkage_policy.wasm.length > 0, true);
assert.equal(report.cryptography.export_review_required, true);
assert.equal(report.cryptography.status, 'review_required');

for (const token of [
  'Apache-2.0', 'AGPL-3.0', 'SPDX', 'MIT', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib',
  'WTFPL', 'GPL', 'LGPL', '动态链接', '静态链接', '字体', '图标', '加密出口',
  'review_required', '商业分发',
]) {
  assert.ok(policy.includes(token), `license policy missing token: ${token}`);
}

const terminfo = report.dependencies.find((entry) => entry.name === 'terminfo');
assert.ok(terminfo, 'terminfo restriction must be visible');
assert.equal(terminfo.scope, 'development_only');
assert.equal(terminfo.release_eligible, false);
assert.match(String(terminfo.license), /WTFPL/);

console.log('license report contract passed');
