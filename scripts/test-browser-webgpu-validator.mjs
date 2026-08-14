import assert from 'node:assert/strict';
import { validateBrowserBenchmark } from './validate-browser-webgpu-benchmark.mjs';

const valid = {
  schema_version: 1,
  status: 'passed',
  secure_context: true,
  navigator_gpu: true,
  adapter_available: true,
  device_created: true,
  fixture: { bytes: 10485760, sha256: '0000000000000000000000000000000000000000000000000000000000000000' },
  scenarios: [
    { id: 'replay-10mb', status: 'passed', samples: 30, throughput_mb_s: 100, passed: true },
    { id: '4k-full-refresh', status: 'passed', samples: 30, fps: 60, passed: true },
    { id: 'ligature-fi-ffi', status: 'proxy_passed', passed: true },
    { id: 'ime-composition-proxy', status: 'proxy_passed', passed: true },
    { id: 'scrollback-million-lines', status: 'proxy_passed', passed: true },
  ],
};

assert.deepEqual(validateBrowserBenchmark(valid), []);
const invalid = structuredClone(valid);
invalid.scenarios[0].samples = 1;
assert.match(validateBrowserBenchmark(invalid).join('\\n'), /30 samples/);
console.log('browser WebGPU validator contract passed');
