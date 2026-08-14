import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { validateRunnerPreflight } from './validate-runner-preflight.mjs';

const ALLOWED_STATUSES = new Set(['passed', 'blocked_environment']);
const ALLOWED_BACKENDS = new Set(['dx12', 'metal', 'vulkan', 'webgpu']);
const ALLOWED_PROFILES = new Set(['debug', 'release']);
const ALLOWED_DEVICES = new Set([
  'windows',
  'macos-intel',
  'macos-apple-silicon',
  'linux',
  'ios-ipados',
  'android',
  'web-pwa',
]);
const RUNNER_FOR_DEVICE = new Map([
  ['windows', 'native_windows'],
  ['linux', 'native_linux'],
  ['macos-intel', 'native_metal'],
  ['macos-apple-silicon', 'native_metal'],
  ['ios-ipados', 'ios_native'],
  ['android', 'android_native'],
  ['web-pwa', 'browser_webgpu'],
]);

function clone(value) {
  return structuredClone(value);
}

function stripBom(text) {
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function validateFarmPreflight(preflight) {
  assert(preflight && typeof preflight === 'object', 'preflight metadata is required');
  const errors = validateRunnerPreflight(preflight);
  assert(errors.length === 0, `preflight invalid: ${errors.join('; ')}`);
  return preflight;
}

function validateFarmResult(result, baseById) {
  assert(result && typeof result === 'object', 'farm result must be an object');
  assert(typeof result.id === 'string' && result.id.length > 0, 'farm result id is required');
  const base = baseById.get(result.id);
  assert(base, `unknown matrix cell: ${result.id}`);
  assert(result.device_class === base.device_class, `device_class mismatch: ${result.id}`);
  assert(result.backend === base.backend, `backend mismatch: ${result.id}`);
  assert(result.profile === base.profile, `profile mismatch: ${result.id}`);
  assert(ALLOWED_STATUSES.has(result.status), `invalid status: ${result.id}`);
  assert(ALLOWED_BACKENDS.has(result.backend), `invalid backend: ${result.id}`);
  assert(ALLOWED_PROFILES.has(result.profile), `invalid profile: ${result.id}`);
  assert(ALLOWED_DEVICES.has(result.device_class), `invalid device class: ${result.id}`);
  assert(Number.isInteger(result.exit_code), `exit_code must be an integer: ${result.id}`);
  assert(result.status === 'passed' ? result.exit_code === 0 : result.exit_code === 2, `exit_code/status mismatch: ${result.id}`);
  assert(typeof result.command === 'string' && result.command.length > 0, `command is required: ${result.id}`);
  assert(typeof result.report_file === 'string' && result.report_file.length > 0, `report_file is required: ${result.id}`);
  assert(typeof result.trace_dir === 'string' && result.trace_dir.length > 0, `trace_dir is required: ${result.id}`);
  if (result.status === 'blocked_environment') {
    assert(typeof result.blocked_reason === 'string' && result.blocked_reason.length > 0, `blocked_reason is required: ${result.id}`);
  }
  return base;
}

function assertRunnerAvailable(preflight, result) {
  if (result.status !== 'passed') return;
  const runnerName = RUNNER_FOR_DEVICE.get(result.device_class);
  assert(runnerName, `no runner mapping for device class: ${result.device_class}`);
  assert(preflight.runners?.[runnerName]?.available === true, `runner unavailable for passed result: ${result.id} (${runnerName})`);
}

export function mergeReports(baseReport, farmReport) {
  assert(baseReport && typeof baseReport === 'object', 'base report must be an object');
  assert(farmReport && typeof farmReport === 'object', 'farm report must be an object');
  assert(baseReport.schema_version === 1, 'base report schema_version must be 1');
  assert(farmReport.schema_version === 1, 'farm report schema_version must be 1');
  const preflight = validateFarmPreflight(farmReport.preflight);
  assert(baseReport.fixture && farmReport.fixture, 'fixture metadata is required');
  assert(baseReport.fixture.bytes === farmReport.fixture.bytes && baseReport.fixture.sha256.toLowerCase() === farmReport.fixture.sha256.toLowerCase(), 'fixture mismatch');
  assert(Array.isArray(baseReport.results) && baseReport.results.length === 56, 'base report must contain 56 matrix cells');
  assert(Array.isArray(farmReport.results) && farmReport.results.length > 0, 'farm report must contain at least one result');

  const baseById = new Map(baseReport.results.map((result) => [result.id, result]));
  const seen = new Set();
  const merged = clone(baseReport);
  const replaced = [];
  for (const result of farmReport.results) {
    assert(!seen.has(result.id), `duplicate farm result: ${result.id}`);
    seen.add(result.id);
    validateFarmResult(result, baseById);
    assertRunnerAvailable(preflight, result);
    const index = merged.results.findIndex((candidate) => candidate.id === result.id);
    assert(index >= 0, `unknown matrix cell: ${result.id}`);
    merged.results[index] = clone(result);
    replaced.push(result.id);
  }

  const blocked = merged.results.some((result) => result.status === 'blocked_environment');
  const failed = merged.results.some((result) => !ALLOWED_STATUSES.has(result.status));
  merged.status = failed ? 'failed' : (blocked ? 'complete_with_blocked_environment' : 'passed');
  merged.device_farm = {
    schema_version: 1,
    merged_at_utc: new Date().toISOString(),
    source: clone(farmReport.source ?? {}),
    preflight: clone(preflight),
    results_replaced: replaced,
  };
  return merged;
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1].replaceAll('\\\\', '/')}`).href) {
  const [basePath, farmPath, outputPath] = process.argv.slice(2);
  if (!basePath || !farmPath || !outputPath) {
    console.error('Usage: node scripts/merge-wgpu-device-farm.mjs <base-report.json> <farm-report.json> <output-report.json>');
    process.exit(64);
  }
  try {
    const base = JSON.parse(stripBom(readFileSync(resolve(basePath), 'utf8')));
    const farm = JSON.parse(stripBom(readFileSync(resolve(farmPath), 'utf8')));
    const merged = mergeReports(base, farm);
    const output = resolve(outputPath);
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(output, `${JSON.stringify(merged, null, 2)}\n`, 'utf8');
    console.log(`wgpu device-farm merge valid: replaced=${merged.device_farm.results_replaced.length}, status=${merged.status}, output=${output}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
