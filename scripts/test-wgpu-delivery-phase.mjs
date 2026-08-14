import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const reportText = readFileSync('docs/reports/WGPU_FEASIBILITY.json', 'utf8');
const report = JSON.parse(reportText.charCodeAt(0) === 0xfeff ? reportText.slice(1) : reportText);
const reserved = [
  'macos-intel',
  'macos-apple-silicon',
  'linux',
  'ios-ipados',
  'android',
  'web-pwa',
];

assert.equal(report.delivery_phase, 'windows-first');
assert.deepEqual(report.current_release_platforms, ['windows']);
assert.deepEqual(report.reserved_device_classes, reserved);
assert.equal(report.interface_reservation.protocol_schema, 'protocol/schema/domain-v1.json');
assert.equal(report.interface_reservation.dependency_rules, 'architecture/dependency-rules.json');
assert.equal(report.interface_reservation.renderer_contract, 'docs/ADR-004-terminal-abstraction.md');
assert.equal(report.interface_reservation.abi_boundary, 'docs/ADR-002-module-boundaries-and-dependencies.md');
assert.equal(report.windows_first.accepted_native_cells, 4);
assert.equal(report.windows_first.required_native_backends.length, 2);
assert.deepEqual(report.windows_first.required_native_backends, ['dx12', 'vulkan']);
assert.equal(report.windows_first.web_reference, 'browser_webgpu');

console.log('wgpu delivery phase contract passed');
