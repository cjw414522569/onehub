import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { mergeReports } from './merge-wgpu-device-farm.mjs';

const root = process.cwd();
const baseText = readFileSync(join(root, 'docs/reports/WGPU_FEASIBILITY.json'), 'utf8');
const base = JSON.parse(baseText.charCodeAt(0) === 0xfeff ? baseText.slice(1) : baseText);
const preflightText = readFileSync(join(root, 'artifacts/perf/wgpu-feasibility/runner-preflight.json'), 'utf8');
const preflight = JSON.parse(preflightText.charCodeAt(0) === 0xfeff ? preflightText.slice(1) : preflightText);
const source = base.results.find((result) => result.id === 'windows-dx12-release');
const farm = { schema_version: 1, fixture: base.fixture, source: { runner_id: 'contract-test' }, preflight, results: [source] };
const merged = mergeReports(base, farm);
assert.equal(merged.results.filter((result) => result.id === source.id).length, 1);
assert.equal(merged.device_farm.results_replaced.length, 1);
assert.equal(merged.status, 'complete_with_blocked_environment');
assert.equal(merged.device_farm.preflight.status, preflight.status);

assert.throws(() => mergeReports(base, { ...farm, preflight: undefined }), /preflight metadata is required/);
assert.throws(() => mergeReports(base, { ...farm, preflight: { ...preflight, matrix: { ...preflight.matrix, complete: true } } }), /preflight invalid/);

assert.throws(() => mergeReports(base, { ...farm, results: [{ ...source, id: 'unknown-cell' }] }), /unknown matrix cell/);
assert.throws(() => mergeReports(base, { ...farm, results: [source, source] }), /duplicate farm result/);
assert.throws(() => mergeReports(base, { ...farm, fixture: { ...base.fixture, sha256: '0'.repeat(64) } }), /fixture mismatch/);

const temp = mkdtempSync(join(tmpdir(), 'wgpu-device-farm-merge-'));
const farmPath = join(temp, 'farm.json');
const outputPath = join(temp, 'merged.json');
writeFileSync(farmPath, JSON.stringify(farm));
rmSync(temp, { recursive: true, force: true });
console.log('wgpu device-farm merge contract passed');
