import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const path = 'docs/ADR-005-wgpu-backend-glyph-and-recovery-strategy.md';
assert.equal(existsSync(path), true, 'ADR-005 must exist');
const text = readFileSync(path, 'utf8');

for (const heading of [
  '## 1. Context and evidence',
  '## 2. Decision',
  '## 3. Backend strategy',
  '## 4. Glyph rasterization and shaping',
  '## 5. Software rendering fallback',
  '## 6. Device loss and recovery',
  '## 7. Windows-first delivery boundary',
  '## 8. Validation gates',
  '## 9. Consequences',
]) {
  assert.ok(text.split(/\r?\n/).includes(heading), 'missing heading: ' + heading);
}

for (const token of [
  'ADR-005', 'wgpu 30.0.0', 'DX12', 'Metal', 'Vulkan', 'WebGPU',
  'Windows-first', 'browser_webgpu', 'software', 'device.lost',
  'adapter.request_device', 'queue.submit', 'glyph atlas', 'HarfBuzz',
  'DirectWrite', 'CoreText', 'font fallback', 'IME', 'RSS/private-bytes',
  'blocked_environment', 'protocol/schema/domain-v1.json',
  'architecture/dependency-rules.json', 'docs/ADR-004-terminal-abstraction.md',
  'docs/ADR-002-module-boundaries-and-dependencies.md',
]) {
  assert.ok(text.includes(token), 'missing ADR-005 token: ' + token);
}

for (const backend of ['DX12', 'Metal', 'Vulkan', 'WebGPU']) {
  const row = text.split(/\r?\n/).find((line) => line.startsWith('| ' + backend + ' |'));
  assert.ok(row, 'missing backend decision row: ' + backend);
  assert.ok(row.includes('fallback') || row.includes('Fallback'), 'backend row lacks fallback policy: ' + backend);
}

assert.match(text, /T012/);
assert.match(text, /T013/);
assert.match(text, /72-hour soak/);
assert.match(text, /native shaping/);
assert.match(text, /not convert blocked/i);

console.log('ADR-005 wgpu strategy contract passed');
