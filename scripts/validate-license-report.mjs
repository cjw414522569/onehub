import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { isAbsolute, resolve } from 'node:path';

const ROOT = resolve(process.cwd());
const DEFAULT_REPORT = 'artifacts/reports/LICENSE_COMPLIANCE.json';
const ALLOWED_RESOURCE_STATUSES = new Set(['not_present', 'review_required']);
const ALLOWED_CRYPTO_STATUSES = new Set(['review_required']);
const FORBIDDEN_LICENSE = /(?:^|\b)(GPL|AGPL|LGPL|WTFPL)(?:\b|$)/i;

function fail(message) {
  throw new Error(message);
}

function readJson(file) {
  const bytes = readFileSync(file);
  const text = bytes[0] === 0xff && bytes[1] === 0xfe
    ? bytes.toString('utf16le', 2)
    : bytes.toString('utf8').replace(/^\uFEFF/, '');
  return JSON.parse(text);
}

function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function resolveRepoPath(pathValue) {
  if (typeof pathValue !== 'string' || pathValue.length === 0) return null;
  return isAbsolute(pathValue) ? pathValue : resolve(ROOT, pathValue);
}

function assertHashEvidence(entry, label) {
  if (!entry || entry.exists !== true) fail(`${label} must exist`);
  if (typeof entry.path !== 'string' || !entry.path) fail(`${label} path is missing`);
  if (!/^[0-9a-f]{64}$/i.test(String(entry.sha256 ?? ''))) fail(`${label} sha256 is missing or malformed`);
  const absolute = resolveRepoPath(entry.path);
  if (!absolute || !existsSync(absolute) || !statSync(absolute).isFile()) fail(`${label} file cannot be located: ${entry.path}`);
  if (sha256(absolute) !== String(entry.sha256).toLowerCase()) fail(`${label} hash mismatch: ${entry.path}`);
}

function validateMetadataInputs(report) {
  if (!Array.isArray(report.metadata_inputs) || report.metadata_inputs.length !== 4) fail('metadata inputs must contain four entries');
  for (const input of report.metadata_inputs) {
    if (!input || typeof input.path !== 'string') fail('metadata input path is missing');
    if (input.exists !== true) fail(`metadata input must exist: ${input.path}`);
    const absolute = resolveRepoPath(input.path);
    if (!absolute || !existsSync(absolute) || !statSync(absolute).isFile()) fail(`metadata input file cannot be located: ${input.path}`);
    if (!/^[0-9a-f]{64}$/i.test(String(input.sha256 ?? ''))) fail(`metadata input hash is missing: ${input.path}`);
    if (sha256(absolute) !== String(input.sha256).toLowerCase()) fail(`metadata input hash mismatch: ${input.path}`);
    if (!Number.isInteger(input.package_count) || input.package_count <= 0) fail(`metadata input package_count invalid: ${input.path}`);
  }
}

function validateDependencies(report) {
  if (!Array.isArray(report.dependencies) || report.dependencies.length === 0) fail('dependencies must be a non-empty array');
  for (const dependency of report.dependencies) {
    for (const field of ['name', 'version', 'scope', 'classification']) {
      if (typeof dependency?.[field] !== 'string' || dependency[field].length === 0) fail(`dependency ${field} is missing`);
    }
    if (typeof dependency.license !== 'string' || dependency.license.length === 0) fail(`dependency license is missing: ${dependency.name}@${dependency.version}`);
    if (!['release_candidate', 'development_only', 'transitive'].includes(dependency.scope)) fail(`dependency scope is invalid: ${dependency.name}@${dependency.version}`);
    if (typeof dependency.release_eligible !== 'boolean') fail(`dependency release_eligible is invalid: ${dependency.name}@${dependency.version}`);
    if (FORBIDDEN_LICENSE.test(dependency.license) && dependency.release_eligible) fail(`forbidden copyleft license marked release eligible: ${dependency.name}@${dependency.version} (${dependency.license})`);
    if (dependency.scope === 'release_candidate' && dependency.release_eligible !== true) fail(`release candidate dependency is not release eligible: ${dependency.name}@${dependency.version}`);
    if (dependency.scope !== 'release_candidate' && dependency.release_eligible === true) fail(`non-release dependency marked release eligible: ${dependency.name}@${dependency.version}`);
    if (!dependency.license_evidence || !['located', 'not_located'].includes(dependency.license_evidence.status)) fail(`dependency license evidence is invalid: ${dependency.name}@${dependency.version}`);
    if (typeof dependency.license_evidence.review_required !== 'boolean') fail(`dependency license evidence review flag is invalid: ${dependency.name}@${dependency.version}`);
  }
}

function validateReport(report) {
  if (!report || report.schema_version !== 1) fail('schema_version must be 1');
  if (report.project_license !== 'Apache-2.0') fail('project_license must be Apache-2.0');
  validateMetadataInputs(report);
  validateDependencies(report);
  if (!Array.isArray(report.release_blockers)) fail('release_blockers must be an array');
  if (report.release_blockers.length === 0 && report.status !== 'pass_with_restrictions') fail('zero release blockers requires pass_with_restrictions status');
  if (report.release_blockers.length > 0 && report.status === 'pass_with_restrictions') fail('release blockers cannot have pass_with_restrictions status');
  assertHashEvidence(report.distribution?.project_license_file, 'LICENSE');
  assertHashEvidence(report.distribution?.notice_file, 'NOTICE');
  if (!report.distribution?.third_party_report?.exists) fail('third-party report evidence must exist');
  assertHashEvidence(report.distribution.third_party_report, 'third-party report');
  for (const linkage of ['static', 'dynamic', 'wasm']) {
    if (typeof report.distribution?.linkage_policy?.[linkage] !== 'string' || report.distribution.linkage_policy[linkage].trim().length === 0) fail(`linkage policy ${linkage} is missing`);
  }
  if (!ALLOWED_RESOURCE_STATUSES.has(report.resources?.status)) fail(`resources.status is invalid: ${report.resources?.status}`);
  if (report.cryptography?.export_review_required !== true) fail('cryptography.export_review_required must be true');
  if (!ALLOWED_CRYPTO_STATUSES.has(report.cryptography?.status)) fail(`cryptography status must remain review_required: ${report.cryptography?.status}`);
  return report;
}

const reportPath = process.argv[2] ? resolveRepoPath(process.argv[2]) : resolve(ROOT, DEFAULT_REPORT);
if (!reportPath || !existsSync(reportPath)) {
  console.error(`license report not found: ${process.argv[2] || DEFAULT_REPORT}`);
  process.exit(1);
}

try {
  validateReport(readJson(reportPath));
  console.log(`license report validated: ${process.argv[2] || DEFAULT_REPORT}`);
} catch (error) {
  console.error(`license report validation failed: ${error.message}`);
  process.exit(1);
}
