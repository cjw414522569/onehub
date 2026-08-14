import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { detectRunnerPreflight } from './runner-preflight.mjs';

const missing = detectRunnerPreflight({
  platform: 'win32',
  env: {},
  commands: {},
  browsers: {},
  androidDevices: [],
  hostCapabilities: {
    containers: { docker: false, podman: false, qemu: false },
    linux_compat: { wsl: false },
    mobile_sdk_tools: { emulator: false, avdmanager: false, sdkmanager: false, xcodebuild: false, swift: false },
  },
});

assert.equal(missing.schema_version, 1);
assert.equal(missing.status, 'blocked_environment');
assert.equal(missing.platform, 'win32');
assert.equal(missing.tools.rustc.available, false);
assert.equal(missing.devices.android.connected, false);
assert.equal(missing.devices.ios.connected, false);
assert.equal(missing.runners.native_metal.available, false);
assert.equal(missing.host_capabilities.containers.docker, false);
assert.equal(missing.host_capabilities.linux_compat.wsl, false);

const partial = detectRunnerPreflight({
  platform: 'darwin',
  env: { DEVELOPER_DIR: '/Applications/Xcode.app' },
  commands: {
    rustc: { available: true, version: 'rustc 1.97.1' },
    cargo: { available: true, version: 'cargo 1.97.1' },
    xcrun: { available: true, version: 'xcrun 1' },
    adb: { available: true, version: 'Android Debug Bridge version 1' },
  },
  browsers: { chrome: { available: true, path: '/Applications/Google Chrome.app' } },
  androidDevices: [{ serial: 'emulator-5554', state: 'device' }],
  iosDevices: [{ identifier: 'ios-1', state: 'connected' }],
  hostCapabilities: {
    containers: { docker: true, podman: false, qemu: true },
    linux_compat: { wsl: true },
    mobile_sdk_tools: { emulator: true, avdmanager: true, sdkmanager: true, xcodebuild: true, swift: true },
  },
});

assert.equal(partial.status, 'partial');
assert.equal(partial.matrix.complete, false);
assert.equal(partial.devices.android.connected, true);
assert.equal(partial.devices.ios.connected, true);
assert.equal(partial.runners.native_metal.available, true);
assert.equal(partial.runners.browser_webgpu.available, true);
assert.equal(partial.host_capabilities.containers.qemu, true);
assert.equal(partial.host_capabilities.mobile_sdk_tools.xcodebuild, true);

const outputDir = mkdtempSync(join(tmpdir(), 'runner-preflight-cli-'));
const outputPath = join(outputDir, 'preflight.json');
const cli = spawnSync(process.execPath, ['scripts/runner-preflight.mjs', '--output', outputPath], { cwd: process.cwd(), encoding: 'utf8' });
assert.equal(cli.status, 2, `${cli.stdout}\n${cli.stderr}`);
assert.equal(JSON.parse(readFileSync(outputPath, 'utf8')).schema_version, 1);
rmSync(outputDir, { recursive: true, force: true });

console.log('runner preflight contract passed');
