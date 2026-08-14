import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

const DEFAULT_OUTPUT = 'artifacts/perf/wgpu-feasibility/device-farm-bundle.json';
const REQUIRED_FILES = [
  ['spikes/wgpu-terminal/fixtures/replay-10mb.bin', 'fixed replay fixture'],
  ['spikes/wgpu-terminal/fixtures/manifest.json', 'fixture manifest'],
  ['docs/reports/WGPU_FEASIBILITY.json', 'base feasibility report'],
  ['docs/reports/WGPU_FEASIBILITY.md', 'rendered feasibility report'],
  ['artifacts/perf/wgpu-feasibility/runner-preflight.json', 'runner preflight snapshot'],
  ['scripts/run-wgpu-feasibility.ps1', 'native matrix runner'],
  ['scripts/validate-wgpu-feasibility.ps1', 'native matrix validator'],
  ['scripts/merge-wgpu-device-farm.mjs', 'device farm merge tool'],
  ['scripts/validate-wgpu-device-farm-bundle.mjs', 'bundle validator'],
];

function stripBom(text) {
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
}

function readJson(path) {
  return JSON.parse(stripBom(readFileSync(path, 'utf8')));
}

function hashFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function fileEntry(root, path, role) {
  const absolute = resolve(root, path);
  const info = statSync(absolute);
  if (!info.isFile()) throw new Error(`bundle path is not a file: ${path}`);
  return { path, role, bytes: info.size, sha256: hashFile(absolute) };
}

export function buildDeviceFarmBundle(root, generatedAt = new Date().toISOString()) {
  const reportPath = resolve(root, 'docs/reports/WGPU_FEASIBILITY.json');
  const preflightPath = resolve(root, 'artifacts/perf/wgpu-feasibility/runner-preflight.json');
  const report = readJson(reportPath);
  const preflight = readJson(preflightPath);
  const files = REQUIRED_FILES.map(([path, role]) => fileEntry(root, path, role));
  const fixture = report.fixture;
  const requiredResults = report.results.map((result) => result.id);
  const commands = [
    'node scripts/runner-preflight.mjs --output artifacts/perf/wgpu-feasibility/runner-preflight.json',
    'node scripts/validate-runner-preflight.mjs artifacts/perf/wgpu-feasibility/runner-preflight.json',
    'powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-wgpu-feasibility.ps1 -SkipBuild',
    'powershell -NoProfile -ExecutionPolicy Bypass -File scripts/validate-wgpu-feasibility.ps1',
    'node scripts/merge-wgpu-device-farm.mjs docs/reports/WGPU_FEASIBILITY.json farm-results.json merged-report.json',
    'powershell -NoProfile -ExecutionPolicy Bypass -File scripts/validate-wgpu-feasibility.ps1 -ReportFile merged-report.json',
  ];
  return {
    schema_version: 1,
    bundle_id: 'wgpu-device-farm-v1',
    status: preflight.status,
    generated_at_utc: generatedAt,
    fixture: {
      path: 'spikes/wgpu-terminal/fixtures/replay-10mb.bin',
      bytes: fixture.bytes,
      sha256: String(fixture.sha256).toLowerCase(),
    },
    matrix: {
      backends: [...report.backend_matrix],
      device_classes: [...report.device_matrix],
      profiles: ['debug', 'release'],
      scenarios: [...report.scenario_matrix],
      required_results: requiredResults,
    },
    files,
    preflight: {
      path: 'artifacts/perf/wgpu-feasibility/runner-preflight.json',
      status: preflight.status,
      sha256: hashFile(preflightPath),
    },
    commands,
  };
}

function parseArgs(argv) {
  const args = { output: DEFAULT_OUTPUT, root: process.cwd() };
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === '--output') args.output = argv[++index];
    else if (argv[index] === '--root') args.root = argv[++index];
    else if (argv[index] === '--help' || argv[index] === '-h') {
      console.log('Usage: node scripts/generate-wgpu-device-farm-bundle.mjs [--root path] [--output path]');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${argv[index]}`);
    }
  }
  return args;
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1].replaceAll('\\', '/')}`).href) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const root = resolve(args.root);
    const bundle = buildDeviceFarmBundle(root);
    const output = resolve(root, args.output);
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(output, `${JSON.stringify(bundle, null, 2)}\n`, 'utf8');
    console.log(`wgpu device-farm bundle generated: status=${bundle.status}, files=${bundle.files.length}, matrix_cells=${bundle.matrix.required_results.length}, output=${output}`);
  } catch (error) {
    console.error(`${error.name}: ${error.message}`);
    process.exitCode = 1;
  }
}
