import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.cwd());
const REPORT_PATH = resolve(ROOT, 'artifacts/reports/LICENSE_COMPLIANCE.json');
const VALIDATOR_PATH = resolve(ROOT, 'scripts/validate-license-report.mjs');
const THIRD_PARTY_PATH = resolve(ROOT, 'artifacts/reports/THIRD_PARTY_LICENSES.md');

assert.equal(existsSync(REPORT_PATH), true, 'baseline license report must exist');
assert.equal(existsSync(VALIDATOR_PATH), true, 'license report validator must exist');
assert.equal(existsSync(THIRD_PARTY_PATH), true, 'third-party license report must exist');

function readReport() {
  return JSON.parse(readFileSync(REPORT_PATH, 'utf8').replace(/^\uFEFF/, ''));
}

function runValidator(report, label) {
  const tempRoot = mkdtempSync(join(tmpdir(), 'ssh-license-validator-'));
  const fixturePath = join(tempRoot, `${label}.json`);
  writeFileSync(fixturePath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  try {
    return spawnSync(process.execPath, [VALIDATOR_PATH, fixturePath], {
      cwd: ROOT,
      encoding: 'utf8',
    });
  } finally {
    rmSync(tempRoot, { recursive: true, force: true });
  }
}

function cloneReport() {
  return structuredClone(readReport());
}

function expectPass(report, label) {
  const result = runValidator(report, label);
  assert.equal(result.status, 0, `${label} should pass:\n${result.stdout}\n${result.stderr}`);
}

function expectFail(report, label, expectedMessage) {
  const result = runValidator(report, label);
  assert.notEqual(result.status, 0, `${label} should fail`);
  const output = `${result.stdout}\n${result.stderr}`;
  assert.match(output, expectedMessage, `${label} error should identify the violated contract`);
}

const baseline = readReport();
expectPass(baseline, 'baseline');

const thirdParty = readFileSync(THIRD_PARTY_PATH, 'utf8');
for (const token of [
  '# Third-party license inventory',
  '| Name | Version | SPDX/license expression | Scope | Source | Repository | Release eligible | License evidence | Review required |',
  'terminfo',
  'WTFPL',
]) {
  assert.ok(thirdParty.includes(token), `third-party report missing token: ${token}`);
}

const missingMetadata = cloneReport();
missingMetadata.metadata_inputs[0].path = 'artifacts/license-metadata-missing-for-test.json';
missingMetadata.metadata_inputs[0].exists = false;
missingMetadata.metadata_inputs[0].sha256 = null;
expectFail(missingMetadata, 'missing-metadata', /metadata input/i);

const badLicenseHash = cloneReport();
badLicenseHash.distribution.project_license_file.sha256 = '0'.repeat(64);
expectFail(badLicenseHash, 'bad-license-hash', /LICENSE.*hash|project license/i);

const badNoticeHash = cloneReport();
badNoticeHash.distribution.notice_file.sha256 = '0'.repeat(64);
expectFail(badNoticeHash, 'bad-notice-hash', /NOTICE.*hash|notice/i);

const forbiddenReleaseDependency = cloneReport();
const terminfo = forbiddenReleaseDependency.dependencies.find((entry) => entry.name === 'terminfo');
assert.ok(terminfo, 'terminfo must be present for the negative license fixture');
terminfo.scope = 'release_candidate';
terminfo.license = 'WTFPL';
terminfo.classification = 'restricted_copyleft';
terminfo.release_eligible = true;
expectFail(forbiddenReleaseDependency, 'forbidden-release-license', /WTFPL|GPL|release.*eligible/i);

const passedCryptoReview = cloneReport();
passedCryptoReview.cryptography.status = 'passed';
expectFail(passedCryptoReview, 'crypto-review-passed', /cryptography|export.*review|required/i);

const missingDependencyLicense = cloneReport();
missingDependencyLicense.dependencies[0].license = null;
expectFail(missingDependencyLicense, 'missing-dependency-license', /dependency.*license|license.*missing/i);

console.log('license report validator contract passed');
