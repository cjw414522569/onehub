import { createHash } from 'node:crypto';
import { readFileSync, statSync } from 'node:fs';
import { isAbsolute, relative, resolve } from 'node:path';
import { validateRunnerPreflight } from './validate-runner-preflight.mjs';

const REQUIRED_FILES = [
  'spikes/wgpu-terminal/fixtures/replay-10mb.bin',
  'spikes/wgpu-terminal/fixtures/manifest.json',
  'docs/reports/WGPU_FEASIBILITY.json',
  'docs/reports/WGPU_FEASIBILITY.md',
  'artifacts/perf/wgpu-feasibility/runner-preflight.json',
];
const REQUIRED_BACKENDS = ['dx12', 'metal', 'vulkan', 'webgpu'];
const REQUIRED_DEVICES = ['windows', 'macos-intel', 'macos-apple-silicon', 'linux', 'ios-ipados', 'android', 'web-pwa'];
const REQUIRED_PROFILES = ['debug', 'release'];
const REQUIRED_SCENARIOS = ['replay-10mb', '4k-full-refresh', 'ligature-fi-ffi', 'ime-composition-proxy', 'scrollback-million-lines'];

function stripBom(text) {
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
}

function exactSet(actual, expected) {
  return Array.isArray(actual)
    && actual.length === expected.length
    && actual.every((value) => expected.includes(value))
    && expected.every((value) => actual.includes(value));
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function safeResolve(root, relativePath) {
  if (typeof relativePath !== 'string' || relativePath.length === 0) return null;
  if (isAbsolute(relativePath)) return null;
  const rootPath = resolve(root);
  const target = resolve(rootPath, relativePath);
  const escaped = relative(rootPath, target).startsWith('..');
  return escaped ? null : target;
}

function validateFileEntry(entry, root, errors) {
  if (!entry || typeof entry !== 'object') {
    errors.push('file entry must be an object');
    return;
  }
  if (typeof entry.path !== 'string' || entry.path.length === 0) {
    errors.push('file entry path must be non-empty');
    return;
  }
  const target = safeResolve(root, entry.path);
  if (!target) {
    errors.push(`file path escapes bundle root: ${entry.path}`);
    return;
  }
  if (typeof entry.role !== 'string' || entry.role.length === 0) errors.push(`file role is required: ${entry.path}`);
  if (!Number.isInteger(entry.bytes) || entry.bytes < 0) errors.push(`file bytes must be a non-negative integer: ${entry.path}`);
  if (!/^[0-9a-f]{64}$/i.test(String(entry.sha256 ?? ''))) errors.push(`file sha256 must be 64 hex characters: ${entry.path}`);
  try {
    const info = statSync(target);
    if (info.isDirectory()) errors.push(`file path is a directory: ${entry.path}`);
    if (Number(entry.bytes) !== info.size) errors.push(`file byte length mismatch: ${entry.path}`);
    if (String(entry.sha256).toLowerCase() !== sha256(target)) errors.push(`file sha256 mismatch: ${entry.path}`);
  } catch (error) {
    errors.push(`required file is missing or unreadable: ${entry.path} (${error.message})`);
  }
}

export function validateDeviceFarmBundle(bundle, root = process.cwd()) {
  const errors = [];
  if (!bundle || typeof bundle !== 'object' || Array.isArray(bundle)) return ['bundle must be an object'];
  if (bundle.schema_version !== 1) errors.push('schema_version must be 1');
  if (typeof bundle.bundle_id !== 'string' || bundle.bundle_id.length === 0) errors.push('bundle_id must be non-empty');
  if (!['partial', 'ready', 'blocked_environment'].includes(bundle.status)) errors.push('status must be partial, ready, or blocked_environment');
  if (typeof bundle.generated_at_utc !== 'string' || bundle.generated_at_utc.length === 0) errors.push('generated_at_utc must be non-empty');

  if (!bundle.fixture || typeof bundle.fixture !== 'object') {
    errors.push('fixture metadata is required');
  } else {
    if (bundle.fixture.path !== 'spikes/wgpu-terminal/fixtures/replay-10mb.bin') errors.push('fixture path must be the fixed replay fixture');
    if (bundle.fixture.bytes !== 10485760) errors.push('fixture bytes must be 10485760');
    if (!/^[0-9a-f]{64}$/i.test(String(bundle.fixture.sha256 ?? ''))) errors.push('fixture sha256 must be 64 hex characters');
  }

  if (!bundle.matrix || typeof bundle.matrix !== 'object') {
    errors.push('matrix metadata is required');
  } else {
    if (!exactSet(bundle.matrix.backends, REQUIRED_BACKENDS)) errors.push('matrix.backends must equal DX12, Metal, Vulkan, WebGPU');
    if (!exactSet(bundle.matrix.device_classes, REQUIRED_DEVICES)) errors.push('matrix.device_classes must equal the fixed Tier 1/Web set');
    if (!exactSet(bundle.matrix.profiles, REQUIRED_PROFILES)) errors.push('matrix.profiles must equal debug and release');
    if (!exactSet(bundle.matrix.scenarios, REQUIRED_SCENARIOS)) errors.push('matrix.scenarios must equal the fixed five scenarios');
    if (!Array.isArray(bundle.matrix.required_results) || bundle.matrix.required_results.length !== 56) {
      errors.push('matrix.required_results must contain 56 matrix cells');
    } else {
      const unique = new Set(bundle.matrix.required_results);
      if (unique.size !== 56 || [...unique].some((id) => typeof id !== 'string' || id.length === 0)) errors.push('matrix.required_results must contain 56 unique non-empty ids');
    }
  }

  if (!Array.isArray(bundle.files)) {
    errors.push('files must be an array');
  } else {
    const paths = bundle.files.map((entry) => entry?.path).filter((path) => typeof path === 'string');
    if (new Set(paths).size !== paths.length) errors.push('files must not contain duplicate paths');
    for (const entry of bundle.files) validateFileEntry(entry, root, errors);
    for (const required of REQUIRED_FILES) {
      if (!paths.includes(required)) errors.push(`required file is missing from bundle: ${required}`);
    }
  }

  if (!bundle.preflight || typeof bundle.preflight !== 'object') {
    errors.push('preflight metadata is required');
  } else {
    if (typeof bundle.preflight.path !== 'string' || bundle.preflight.path.length === 0) errors.push('preflight.path is required');
    const preflightPath = safeResolve(root, bundle.preflight.path);
    if (!preflightPath) errors.push('preflight.path must remain inside bundle root');
    if (!/^[0-9a-f]{64}$/i.test(String(bundle.preflight.sha256 ?? ''))) errors.push('preflight.sha256 must be 64 hex characters');
    if (bundle.preflight.status !== bundle.status) errors.push('preflight.status must match bundle.status');
    if (preflightPath) {
      try {
        if (String(bundle.preflight.sha256).toLowerCase() !== sha256(preflightPath)) errors.push('preflight.sha256 mismatch');
        const report = JSON.parse(stripBom(readFileSync(preflightPath, 'utf8')));
        const preflightErrors = validateRunnerPreflight(report);
        if (preflightErrors.length > 0) errors.push(`preflight is invalid: ${preflightErrors.join('; ')}`);
      } catch (error) {
        errors.push(`preflight cannot be read: ${error.message}`);
      }
    }
  }

  const reportPath = safeResolve(root, 'docs/reports/WGPU_FEASIBILITY.json');
  if (reportPath) {
    try {
      const report = JSON.parse(stripBom(readFileSync(reportPath, 'utf8')));
      if (report.schema_version !== 1) errors.push('base report schema_version must be 1');
      if (!Array.isArray(report.results) || report.results.length !== 56) errors.push('base report must contain 56 matrix cells');
      else if (bundle.matrix?.required_results && !exactSet(bundle.matrix.required_results, report.results.map((result) => result.id))) errors.push('matrix.required_results does not match base report ids');
      if (bundle.fixture && report.fixture && (bundle.fixture.bytes !== report.fixture.bytes || String(bundle.fixture.sha256).toLowerCase() !== String(report.fixture.sha256).toLowerCase())) errors.push('bundle fixture does not match base report fixture');
    } catch (error) {
      errors.push(`base report cannot be read: ${error.message}`);
    }
  }

  if (!Array.isArray(bundle.commands) || bundle.commands.length === 0 || bundle.commands.some((command) => typeof command !== 'string' || command.length === 0)) errors.push('commands must contain non-empty command strings');
  return errors;
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1].replaceAll('\\', '/')}`).href) {
  const bundlePath = process.argv[2];
  const root = process.argv[3] ?? process.cwd();
  if (!bundlePath) {
    console.error('Usage: node scripts/validate-wgpu-device-farm-bundle.mjs <bundle.json> [root]');
    process.exit(64);
  }
  try {
    const bundleText = readFileSync(bundlePath, 'utf8');
    const bundle = JSON.parse(stripBom(bundleText));
    const errors = validateDeviceFarmBundle(bundle, root);
    if (errors.length > 0) {
      errors.forEach((error) => console.error(error));
      process.exit(1);
    }
    console.log(`wgpu device-farm bundle valid: status=${bundle.status}, files=${bundle.files.length}, matrix_cells=${bundle.matrix.required_results.length}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
