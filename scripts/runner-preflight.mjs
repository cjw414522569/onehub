import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';

const TOOL_NAMES = ['rustc', 'cargo', 'java', 'gradle', 'adb', 'xcrun', 'dotnet', 'msbuild'];
const BROWSER_NAMES = ['chrome', 'edge', 'chromium'];
const ATTACHED_STATES = new Set(['device', 'connected', 'online', 'booted']);

function firstLine(value) {
  return String(value ?? '').split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? '';
}

function normalizeCommand(value) {
  if (!value || typeof value !== 'object') return { available: false, path: null, version: null };
  const available = value.available === true;
  return {
    available,
    path: available && value.path ? String(value.path) : null,
    version: available && value.version ? String(value.version) : null,
  };
}

function normalizeBrowser(value) {
  if (!value || typeof value !== 'object') return { available: false, path: null };
  const available = value.available === true;
  return { available, path: available && value.path ? String(value.path) : null };
}

function normalizeDevices(values) {
  const devices = Array.isArray(values) ? values.map((device) => ({
    identifier: String(device?.identifier ?? device?.serial ?? device?.id ?? 'unknown'),
    state: String(device?.state ?? 'unknown').toLowerCase(),
    kind: String(device?.kind ?? 'unknown'),
  })) : [];
  const attached = devices.filter((device) => ATTACHED_STATES.has(device.state));
  return { connected: attached.length > 0, count: attached.length, devices };
}

function normalizeBrowserCandidates(values) {
  return Object.fromEntries(BROWSER_NAMES.map((name) => [name, normalizeBrowser(values?.[name])]));
}

function normalizeHostCapabilities(value) {
  const group = (names, source) => Object.fromEntries(names.map((name) => [name, source?.[name] === true]));
  return {
    containers: group(['docker', 'podman', 'qemu'], value?.containers),
    linux_compat: group(['wsl'], value?.linux_compat),
    mobile_sdk_tools: group(['emulator', 'avdmanager', 'sdkmanager', 'xcodebuild', 'swift'], value?.mobile_sdk_tools),
  };
}

export function detectRunnerPreflight(input = {}) {
  const platform = String(input.platform ?? 'unknown');
  const commands = Object.fromEntries(TOOL_NAMES.map((name) => [name, normalizeCommand(input.commands?.[name])]));
  const browsers = normalizeBrowserCandidates(input.browsers);
  const android = normalizeDevices(input.androidDevices);
  const ios = normalizeDevices(input.iosDevices);
  const hostCapabilities = normalizeHostCapabilities(input.hostCapabilities);
  const rustReady = commands.rustc.available && commands.cargo.available;
  const browserReady = Object.values(browsers).some((browser) => browser.available);
  const requiredRunnerNames = ['native_windows', 'native_linux', 'native_metal', 'ios_native', 'android_native', 'browser_webgpu'];
  const runners = {
    native_windows: { available: platform === 'win32' && rustReady, reason: platform !== 'win32' ? 'host is not Windows' : (!rustReady ? 'rustc and cargo are required' : null) },
    native_linux: { available: platform === 'linux' && rustReady, reason: platform !== 'linux' ? 'host is not Linux' : (!rustReady ? 'rustc and cargo are required' : null) },
    native_metal: { available: platform === 'darwin' && commands.xcrun.available, reason: platform !== 'darwin' ? 'host is not macOS' : (!commands.xcrun.available ? 'xcrun is unavailable' : null) },
    ios_native: { available: platform === 'darwin' && commands.xcrun.available && ios.connected, reason: platform !== 'darwin' ? 'host is not macOS' : (!commands.xcrun.available ? 'xcrun is unavailable' : (!ios.connected ? 'no attached iOS device or booted simulator' : null)) },
    android_native: { available: commands.adb.available && android.connected, reason: !commands.adb.available ? 'adb is unavailable' : (!android.connected ? 'no attached Android device' : null) },
    browser_webgpu: { available: browserReady, reason: browserReady ? null : 'Chrome, Edge, or Chromium is unavailable' },
  };
  const availableRunners = Object.entries(runners).filter(([, runner]) => runner.available).map(([name]) => name);
  const missingRunners = Object.entries(runners).filter(([, runner]) => !runner.available).map(([name]) => name);
  const complete = requiredRunnerNames.every((name) => runners[name]?.available === true);
  const blockers = [];
  if (!rustReady) blockers.push('rustc and cargo are required for the shared Rust runner');
  if (missingRunners.length > 0) blockers.push(`missing runner coverage: ${missingRunners.join(', ')}`);
  return {
    schema_version: 1,
    status: availableRunners.length === 0 ? 'blocked_environment' : (complete ? 'ready' : 'partial'),
    platform,
    environment: { developer_dir: input.env?.DEVELOPER_DIR ?? null },
    tools: commands,
    browsers,
    devices: { android, ios },
    host_capabilities: hostCapabilities,
    runners,
    matrix: {
      required_device_classes: ['windows', 'macos-intel', 'macos-apple-silicon', 'linux', 'ios-ipados', 'android', 'web-pwa'],
      complete,
      available_runners: availableRunners,
      missing_runners: missingRunners,
    },
    blockers,
    generated_at_utc: input.generated_at_utc ?? null,
  };
}

function lookupCommand(name) {
  const lookup = process.platform === 'win32' ? 'where.exe' : 'which';
  const result = spawnSync(lookup, [name], { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) return null;
  return firstLine(result.stdout);
}

function probeCommand(name) {
  const path = lookupCommand(name);
  if (!path) return { available: false, path: null, version: null };
  const result = spawnSync(path, ['--version'], { encoding: 'utf8', windowsHide: true });
  return { available: true, path, version: firstLine(result.stdout || result.stderr) || null };
}

function browserCandidates(name) {
  if (process.platform === 'win32') {
    const roots = [process.env.PROGRAMFILES, process.env['PROGRAMFILES(X86)'], process.env.LOCALAPPDATA].filter(Boolean);
    const suffixes = {
      chrome: ['Google\\Chrome\\Application\\chrome.exe'],
      edge: ['Microsoft\\Edge\\Application\\msedge.exe'],
      chromium: ['Chromium\\Application\\chrome.exe'],
    }[name] ?? [];
    return roots.flatMap((root) => suffixes.map((suffix) => `${root}\\${suffix}`));
  }
  if (process.platform === 'darwin') {
    return {
      chrome: ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'],
      edge: ['/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge'],
      chromium: ['/Applications/Chromium.app/Contents/MacOS/Chromium'],
    }[name] ?? [];
  }
  return {
    chrome: ['/usr/bin/google-chrome', '/usr/bin/google-chrome-stable'],
    edge: ['/usr/bin/microsoft-edge'],
    chromium: ['/usr/bin/chromium', '/usr/bin/chromium-browser'],
  }[name] ?? [];
}

function probeBrowser(name) {
  const commandPath = lookupCommand(name);
  const candidatePath = browserCandidates(name).find((candidate) => existsSync(candidate));
  const path = commandPath || candidatePath || null;
  return { available: Boolean(path), path };
}

function probeAvailable(name) {
  return Boolean(lookupCommand(name));
}

function probeWsl() {
  const path = lookupCommand(process.platform === 'win32' ? 'wsl.exe' : 'wsl');
  if (!path) return false;
  const result = spawnSync(path, ['--list', '--quiet'], { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) return false;
  const output = String(result.stdout ?? '').replaceAll('\0', '').trim();
  return output.length > 0 && !/^wsl\.exe\s+--/i.test(output);
}

function discoverAndroid(adb) {
  if (!adb.available || !adb.path) return [];
  const result = spawnSync(adb.path, ['devices'], { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) return [];
  return String(result.stdout ?? '').split(/\r?\n/).slice(1).map((line) => line.trim()).filter(Boolean).flatMap((line) => {
    const [serial, state] = line.split(/\s+/);
    return serial && state ? [{ serial, state, kind: 'android-device' }] : [];
  });
}

function discoverIos(xcrun) {
  if (!xcrun.available || !xcrun.path || process.platform !== 'darwin') return [];
  const result = spawnSync(xcrun.path, ['simctl', 'list', 'devices', 'available', '--json'], { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) return [];
  try {
    const payload = JSON.parse(result.stdout);
    return Object.values(payload.devices ?? {}).flatMap((devices) => Array.isArray(devices) ? devices : []).map((device) => ({
      identifier: device.udid ?? device.name ?? 'unknown',
      state: String(device.state ?? 'unknown').toLowerCase(),
      kind: 'ios-simulator',
    }));
  } catch {
    return [];
  }
}

export function collectRunnerPreflight() {
  const commands = Object.fromEntries(TOOL_NAMES.map((name) => [name, probeCommand(name)]));
  const browsers = Object.fromEntries(BROWSER_NAMES.map((name) => [name, probeBrowser(name)]));
  const hostCapabilities = {
    containers: {
      docker: probeAvailable('docker'),
      podman: probeAvailable('podman'),
      qemu: probeAvailable('qemu-system-x86_64') || probeAvailable('qemu-system-aarch64'),
    },
    linux_compat: { wsl: probeWsl() },
    mobile_sdk_tools: {
      emulator: probeAvailable('emulator'),
      avdmanager: probeAvailable('avdmanager'),
      sdkmanager: probeAvailable('sdkmanager'),
      xcodebuild: probeAvailable('xcodebuild'),
      swift: probeAvailable('swift'),
    },
  };
  return detectRunnerPreflight({
    platform: process.platform,
    env: { DEVELOPER_DIR: process.env.DEVELOPER_DIR ?? null },
    commands,
    browsers,
    androidDevices: discoverAndroid(commands.adb),
    iosDevices: discoverIos(commands.xcrun),
    hostCapabilities,
    generated_at_utc: new Date().toISOString(),
  });
}

function parseArgs(argv) {
  const args = { output: null };
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === '--output') args.output = argv[++index];
    else if (argv[index] === '--help' || argv[index] === '-h') {
      console.log('Usage: node scripts/runner-preflight.mjs [--output path]');
      process.exit(0);
    } else throw new Error(`unknown argument: ${argv[index]}`);
  }
  return args;
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1].replaceAll('\\', '/')}`).href) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const report = collectRunnerPreflight();
    if (args.output) {
      const output = resolve(args.output);
      const { mkdirSync, writeFileSync } = await import('node:fs');
      mkdirSync(dirname(output), { recursive: true });
      writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
    }
    console.log(JSON.stringify(report, null, 2));
    process.exitCode = report.status === 'ready' ? 0 : 2;
  } catch (error) {
    console.error(`${error.name}: ${error.message}`);
    process.exitCode = 1;
  }
}
