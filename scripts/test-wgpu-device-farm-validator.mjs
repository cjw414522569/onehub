import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

const root = process.cwd();
const stripBom = (text) => text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
const base = JSON.parse(stripBom(readFileSync(join(root, 'docs/reports/WGPU_FEASIBILITY.json'), 'utf8')));
const preflight = JSON.parse(stripBom(readFileSync(join(root, 'artifacts/perf/wgpu-feasibility/runner-preflight.json'), 'utf8')));
const farmPreflight = structuredClone(preflight);
farmPreflight.runners.native_metal = { available: true, reason: null };
farmPreflight.matrix.available_runners = [...farmPreflight.matrix.available_runners, 'native_metal'];
farmPreflight.matrix.missing_runners = farmPreflight.matrix.missing_runners.filter((name) => name !== 'native_metal');
farmPreflight.blockers = ['missing runner coverage: native_linux, ios_native, android_native'];
const extra = structuredClone(base.results.find((result) => result.id === 'macos-intel-metal-release'));
const source = base.results.find((result) => result.id === 'windows-dx12-release');
for (const field of ['command', 'report_file', 'trace_dir', 'trace_files', 'stdout']) extra[field] = source[field];
extra.status = 'passed';
extra.exit_code = 0;
extra.blocked_reason = '';
base.results[base.results.findIndex((result) => result.id === extra.id)] = extra;
base.device_farm = {
  schema_version: 1,
  merged_at_utc: new Date().toISOString(),
  source: { runner_id: 'validator-contract' },
  preflight: farmPreflight,
  results_replaced: [extra.id],
};

const temp = mkdtempSync(join(tmpdir(), 'wgpu-device-farm-validator-'));
const reportPath = join(temp, 'merged.json');
writeFileSync(reportPath, JSON.stringify(base));
try {
  const result = spawnSync('powershell.exe', [
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
    join(root, 'scripts/validate-wgpu-feasibility.ps1'),
    '-ReportFile', reportPath,
  ], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  console.log('wgpu device-farm validator integration passed');
} finally {
  rmSync(temp, { recursive: true, force: true });
}

const missingPreflight = structuredClone(base);
delete missingPreflight.device_farm.preflight;
const missingTemp = mkdtempSync(join(tmpdir(), 'wgpu-device-farm-validator-missing-preflight-'));
const missingReportPath = join(missingTemp, 'merged.json');
writeFileSync(missingReportPath, JSON.stringify(missingPreflight));
try {
  const result = spawnSync('powershell.exe', [
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
    join(root, 'scripts/validate-wgpu-feasibility.ps1'),
    '-ReportFile', missingReportPath,
  ], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 1, `${result.stdout}\n${result.stderr}`);
  assert.match(`${result.stdout}\n${result.stderr}`, /device_farm\.preflight is required/);
} finally {
  rmSync(missingTemp, { recursive: true, force: true });
}
