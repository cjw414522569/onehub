import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { validateDeviceFarmBundle } from './validate-wgpu-device-farm-bundle.mjs';

const bundle = JSON.parse(readFileSync('artifacts/perf/wgpu-feasibility/device-farm-bundle.json', 'utf8'));
assert.deepEqual(validateDeviceFarmBundle(bundle, process.cwd()), []);

const missingFile = structuredClone(bundle);
missingFile.files = missingFile.files.filter((file) => file.path !== 'spikes/wgpu-terminal/fixtures/replay-10mb.bin');
assert.match(validateDeviceFarmBundle(missingFile, process.cwd()).join('\n'), /required file/);

const wrongHash = structuredClone(bundle);
wrongHash.files.find((file) => file.path === 'spikes/wgpu-terminal/fixtures/replay-10mb.bin').sha256 = '0'.repeat(64);
assert.match(validateDeviceFarmBundle(wrongHash, process.cwd()).join('\n'), /sha256 mismatch/);

const incompleteMatrix = structuredClone(bundle);
incompleteMatrix.matrix.required_results = incompleteMatrix.matrix.required_results.slice(1);
assert.match(validateDeviceFarmBundle(incompleteMatrix, process.cwd()).join('\n'), /56 matrix cells/);

console.log('wgpu device-farm bundle contract passed');
