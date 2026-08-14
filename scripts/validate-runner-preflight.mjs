import { readFileSync } from 'node:fs';

const TOOL_NAMES = ['rustc', 'cargo', 'java', 'gradle', 'adb', 'xcrun', 'dotnet', 'msbuild'];
const BROWSER_NAMES = ['chrome', 'edge', 'chromium'];
const RUNNER_NAMES = ['native_windows', 'native_linux', 'native_metal', 'ios_native', 'android_native', 'browser_webgpu'];
const DEVICE_CLASSES = ['windows', 'macos-intel', 'macos-apple-silicon', 'linux', 'ios-ipados', 'android', 'web-pwa'];
const ATTACHED_STATES = new Set(['device', 'connected', 'online', 'booted']);
const HOST_CAPABILITY_GROUPS = {
  containers: ['docker', 'podman', 'qemu'],
  linux_compat: ['wsl'],
  mobile_sdk_tools: ['emulator', 'avdmanager', 'sdkmanager', 'xcodebuild', 'swift'],
};

function hasOwn(value, key) {
  return value !== null && typeof value === 'object' && Object.prototype.hasOwnProperty.call(value, key);
}

function exactSet(actual, expected) {
  return Array.isArray(actual) && actual.length === expected.length && expected.every((value) => actual.includes(value)) && actual.every((value) => expected.includes(value));
}

function validateCommand(value, context, errors) {
  if (!value || typeof value !== 'object') {
    errors.push(`${context} must be an object`);
    return;
  }
  if (typeof value.available !== 'boolean') errors.push(`${context}.available must be boolean`);
  if (value.available) {
    if (typeof value.path !== 'string' || value.path.length === 0) errors.push(`${context}.path must be non-empty when available`);
    if (typeof value.version !== 'string' || value.version.length === 0) errors.push(`${context}.version must be non-empty when available`);
  } else if (value.path !== null || value.version !== null) {
    errors.push(`${context}.path and version must be null when unavailable`);
  }
}

function validateBrowser(value, context, errors) {
  if (!value || typeof value !== 'object') {
    errors.push(`${context} must be an object`);
    return;
  }
  if (typeof value.available !== 'boolean') errors.push(`${context}.available must be boolean`);
  if (value.available && (typeof value.path !== 'string' || value.path.length === 0)) errors.push(`${context}.path must be non-empty when available`);
  if (!value.available && value.path !== null) errors.push(`${context}.path must be null when unavailable`);
}

function validateDevices(value, context, errors) {
  if (!value || typeof value !== 'object') {
    errors.push(`${context} must be an object`);
    return { connected: false, count: 0, devices: [] };
  }
  if (typeof value.connected !== 'boolean') errors.push(`${context}.connected must be boolean`);
  if (!Number.isInteger(value.count) || value.count < 0) errors.push(`${context}.count must be a non-negative integer`);
  if (!Array.isArray(value.devices)) {
    errors.push(`${context}.devices must be an array`);
    return { connected: Boolean(value.connected), count: Number.isInteger(value.count) ? value.count : 0, devices: [] };
  }
  const attached = value.devices.filter((device) => device && ATTACHED_STATES.has(String(device.state ?? '').toLowerCase()));
  if (value.count !== attached.length) errors.push(`${context}.count does not match attached device count`);
  if (value.connected !== (attached.length > 0)) errors.push(`${context}.connected does not match attached device count`);
  for (const [index, device] of value.devices.entries()) {
    if (!device || typeof device !== 'object') errors.push(`${context}.devices[${index}] must be an object`);
    else {
      if (typeof device.identifier !== 'string' || device.identifier.length === 0) errors.push(`${context}.devices[${index}].identifier must be non-empty`);
      if (typeof device.state !== 'string' || device.state.length === 0) errors.push(`${context}.devices[${index}].state must be non-empty`);
      if (typeof device.kind !== 'string' || device.kind.length === 0) errors.push(`${context}.devices[${index}].kind must be non-empty`);
    }
  }
  return { connected: Boolean(value.connected), count: Number.isInteger(value.count) ? value.count : 0, devices: value.devices };
}

function validateHostCapabilities(value, errors) {
  if (!value || typeof value !== 'object') {
    errors.push('host_capabilities must be an object');
    return;
  }
  for (const [group, names] of Object.entries(HOST_CAPABILITY_GROUPS)) {
    const capabilities = value[group];
    if (!capabilities || typeof capabilities !== 'object') {
      errors.push(`host_capabilities.${group} must be an object`);
      continue;
    }
    for (const name of names) {
      if (typeof capabilities[name] !== 'boolean') errors.push(`host_capabilities.${group}.${name} must be boolean`);
    }
  }
}

export function validateRunnerPreflight(report) {
  const errors = [];
  if (!report || typeof report !== 'object' || Array.isArray(report)) return ['report must be an object'];
  if (report.schema_version !== 1) errors.push('schema_version must be 1');
  if (!['blocked_environment', 'partial', 'ready'].includes(report.status)) errors.push('status must be blocked_environment, partial, or ready');
  if (typeof report.platform !== 'string' || report.platform.length === 0) errors.push('platform must be non-empty');
  validateHostCapabilities(report.host_capabilities, errors);

  if (!report.tools || typeof report.tools !== 'object') errors.push('tools must be an object');
  else {
    for (const name of TOOL_NAMES) {
      if (!hasOwn(report.tools, name)) errors.push(`missing tool: ${name}`);
      else validateCommand(report.tools[name], `tools.${name}`, errors);
    }
  }

  if (!report.browsers || typeof report.browsers !== 'object') errors.push('browsers must be an object');
  else {
    for (const name of BROWSER_NAMES) {
      if (!hasOwn(report.browsers, name)) errors.push(`missing browser: ${name}`);
      else validateBrowser(report.browsers[name], `browsers.${name}`, errors);
    }
  }

  if (!report.devices || typeof report.devices !== 'object') errors.push('devices must be an object');
  else {
    validateDevices(report.devices.android, 'devices.android', errors);
    validateDevices(report.devices.ios, 'devices.ios', errors);
  }

  if (!report.runners || typeof report.runners !== 'object') errors.push('runners must be an object');
  else {
    for (const name of RUNNER_NAMES) {
      if (!hasOwn(report.runners, name)) {
        errors.push(`missing runner: ${name}`);
        continue;
      }
      const runner = report.runners[name];
      if (!runner || typeof runner !== 'object') {
        errors.push(`runners.${name} must be an object`);
        continue;
      }
      if (typeof runner.available !== 'boolean') errors.push(`runners.${name}.available must be boolean`);
      if (runner.available) {
        if (runner.reason !== null && runner.reason !== '') errors.push(`runners.${name}.reason must be null when available`);
      } else if (typeof runner.reason !== 'string' || runner.reason.length === 0) {
        errors.push(`runners.${name}.reason must explain why unavailable`);
      }
    }
  }

  const availableRunners = RUNNER_NAMES.filter((name) => report.runners?.[name]?.available === true);
  const missingRunners = RUNNER_NAMES.filter((name) => report.runners?.[name]?.available !== true);
  if (!report.matrix || typeof report.matrix !== 'object') errors.push('matrix must be an object');
  else {
    if (!exactSet(report.matrix.required_device_classes, DEVICE_CLASSES)) errors.push('matrix.required_device_classes must equal the fixed device class set');
    if (!exactSet(report.matrix.available_runners, availableRunners)) errors.push('matrix.available_runners does not match runner availability');
    if (!exactSet(report.matrix.missing_runners, missingRunners)) errors.push('matrix.missing_runners does not match runner availability');
    const complete = missingRunners.length === 0;
    if (report.matrix.complete !== complete) errors.push('matrix.complete does not match runner availability');
  }

  const complete = missingRunners.length === 0;
  const expectedStatus = availableRunners.length === 0 ? 'blocked_environment' : (complete ? 'ready' : 'partial');
  if (report.status !== expectedStatus) errors.push(`status must be ${expectedStatus} for the reported runner availability`);
  if (!Array.isArray(report.blockers)) errors.push('blockers must be an array');
  else if (complete && report.blockers.length !== 0) errors.push('blockers must be empty when matrix is complete');
  else if (!complete && report.blockers.length === 0) errors.push('blockers must explain missing runner coverage');
  if (report.generated_at_utc !== null && (typeof report.generated_at_utc !== 'string' || report.generated_at_utc.length === 0)) errors.push('generated_at_utc must be null or a non-empty string');
  return errors;
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1].replaceAll('\\', '/')}`).href) {
  const reportPath = process.argv[2];
  if (!reportPath) {
    console.error('Usage: node scripts/validate-runner-preflight.mjs <runner-preflight.json>');
    process.exit(64);
  }
  try {
    const text = readFileSync(reportPath, 'utf8');
    const report = JSON.parse(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
    const errors = validateRunnerPreflight(report);
    if (errors.length > 0) {
      errors.forEach((error) => console.error(error));
      process.exit(1);
    }
    console.log(`runner preflight valid: status=${report.status}, available=${report.matrix.available_runners.length}, missing=${report.matrix.missing_runners.length}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
