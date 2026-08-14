import assert from 'node:assert/strict';
import { summarizeSamples, terminateChild, fixtureMetadata } from './run-browser-webgpu-probe.mjs';

const summary = summarizeSamples([10, 20, 30, 40], 2_000_000);
assert.equal(summary.samples, 4);
assert.equal(summary.p50_ms, 20);
assert.equal(summary.p95_ms, 40);
assert.equal(summary.p99_ms, 40);
assert.equal(summary.mean_ms, 25);
assert.equal(summary.throughput_mb_s, 100);

const child = (await import('node:child_process')).spawn(process.execPath, ['-e', 'setTimeout(() => {}, 100)'], { stdio: 'ignore' });
await terminateChild(child);
assert.equal(child.exitCode !== null || child.signalCode !== null, true);

const { mkdtempSync, writeFileSync, rmSync } = await import('node:fs');
const { tmpdir } = await import('node:os');
const { join } = await import('node:path');
const fixtureDir = mkdtempSync(join(tmpdir(), 'ssh-webgpu-fixture-test-'));
const fixturePath = join(fixtureDir, 'fixture.bin');
writeFileSync(fixturePath, Buffer.from([1, 2, 3, 4]));
const fixture = fixtureMetadata(fixturePath);
assert.equal(fixture.bytes, 4);
assert.equal(fixture.sha256, '9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a');
rmSync(fixtureDir, { recursive: true, force: true });
console.log('browser WebGPU runner unit contract passed');
