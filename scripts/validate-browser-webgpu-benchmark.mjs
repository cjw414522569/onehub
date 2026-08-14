import { createHash } from 'node:crypto';
import { readFileSync, statSync } from 'node:fs';
import { resolve } from 'node:path';

const SCENARIOS = [
  'replay-10mb',
  '4k-full-refresh',
  'ligature-fi-ffi',
  'ime-composition-proxy',
  'scrollback-million-lines',
];

export function validateBrowserBenchmark(report, fixturePath = null) {
  const errors = [];
  if (!report || typeof report !== 'object') return ['report must be an object'];
  if (report.schema_version !== 1) errors.push('schema_version must be 1');
  if (report.status !== 'passed') errors.push('status must be passed');
  for (const field of ['secure_context', 'navigator_gpu', 'adapter_available', 'device_created']) {
    if (report[field] !== true) errors.push(`${field} must be true`);
  }
  if (!report.fixture || typeof report.fixture !== 'object') {
    errors.push('fixture metadata is required');
  } else {
    if (report.fixture.bytes !== 10485760) errors.push('fixture bytes must be 10485760');
    if (!/^[0-9a-f]{64}$/i.test(String(report.fixture.sha256 ?? ''))) errors.push('fixture sha256 must be 64 hex characters');
    if (fixturePath) {
      try {
        const path = resolve(fixturePath);
        const bytes = statSync(path).size;
        const sha256 = createHash('sha256').update(readFileSync(path)).digest('hex');
        if (bytes !== report.fixture.bytes) errors.push('fixture byte length does not match file');
        if (sha256 !== String(report.fixture.sha256).toLowerCase()) errors.push('fixture sha256 does not match file');
      } catch (error) {
        errors.push(`fixture file cannot be read: ${error.message}`);
      }
    }
  }
  const scenarios = Array.isArray(report.scenarios) ? report.scenarios : [];
  if (scenarios.length !== SCENARIOS.length) errors.push('report must contain five scenarios');
  const byId = new Map(scenarios.map((scenario) => [scenario.id, scenario]));
  for (const id of SCENARIOS) {
    if (!byId.has(id)) errors.push(`missing scenario: ${id}`);
  }
  const replay = byId.get('replay-10mb');
  if (replay) {
    if (replay.status !== 'passed' || replay.passed !== true) errors.push('replay scenario must pass');
    if (replay.samples !== 30) errors.push('replay scenario must contain 30 samples');
    if (!(Number(replay.throughput_mb_s) >= 40)) errors.push('replay throughput must be at least 40 MB/s');
  }
  const frame = byId.get('4k-full-refresh');
  if (frame) {
    if (frame.status !== 'passed' || frame.passed !== true) errors.push('4K scenario must pass');
    if (frame.samples !== 30) errors.push('4K scenario must contain 30 samples');
    if (!(Number(frame.fps) >= 60)) errors.push('4K FPS must be at least 60');
  }
  for (const id of ['ligature-fi-ffi', 'ime-composition-proxy', 'scrollback-million-lines']) {
    const scenario = byId.get(id);
    if (scenario && (scenario.status !== 'proxy_passed' || scenario.passed !== true)) errors.push(`${id} must remain explicitly proxy_passed`);
  }
  return errors;
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1].replaceAll('\\\\', '/')}`).href) {
  const reportPath = process.argv[2];
  const fixturePath = process.argv[3] ?? null;
  if (!reportPath) {
    console.error('Usage: node scripts/validate-browser-webgpu-benchmark.mjs <report.json> [fixture.bin]');
    process.exit(64);
  }
  try {
    const reportText = readFileSync(reportPath, 'utf8');
    const report = JSON.parse(reportText.charCodeAt(0) === 0xfeff ? reportText.slice(1) : reportText);
    const errors = validateBrowserBenchmark(report, fixturePath);
    if (errors.length > 0) {
      for (const error of errors) console.error(error);
      process.exit(1);
    }
    console.log(`browser WebGPU benchmark valid: samples=30, replay_threshold=40 MB/s, frame_threshold=60 FPS`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
