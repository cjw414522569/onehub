#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

function read(root, relativePath) {
  return fs.readFileSync(path.join(root, relativePath), 'utf8');
}

function loadJson(root, relativePath, errors) {
  try {
    return JSON.parse(read(root, relativePath));
  } catch (error) {
    errors.push(`${relativePath} is missing or invalid JSON: ${error.message}`);
    return null;
  }
}

function exactVersion(value) {
  return typeof value === 'string' && /^\d+\.\d+(?:\.\d+)(?:[-+][0-9A-Za-z.-]+)?$/.test(value);
}

function commandVersion(command, args) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    windowsHide: true,
    timeout: 5000,
    killSignal: 'SIGTERM',
    maxBuffer: 64 * 1024,
    shell: process.platform === 'win32',
  });
  if (result.error || result.status !== 0 || result.signal) return { status: 'blocked_environment' };
  const output = `${result.stdout ?? ''}\n${result.stderr ?? ''}`.trim();
  return { status: 'available', output };
}

function normalizeVersion(output, tool) {
  if (tool === 'node') return output.match(/v?(\d+\.\d+\.\d+)/)?.[1] ?? output;
  if (tool === 'npm') return output.match(/(\d+\.\d+\.\d+)/)?.[1] ?? output;
  if (tool === 'rust') return output.match(/rustc\s+(\d+\.\d+\.\d+)/)?.[1] ?? output;
  if (tool === 'dotnet') return output.match(/(\d+\.\d+\.\d+)/)?.[1] ?? output;
  return output;
}

export function validateToolchainModel(model) {
  const errors = [];
  if (!model || model.schema_version !== 1) errors.push('toolchain lock schema_version must be 1.');
  for (const [group, values] of Object.entries(model ?? {})) {
    if (!values || typeof values !== 'object' || Array.isArray(values)) continue;
    for (const [key, value] of Object.entries(values)) {
      if (['status', 'verification', 'manifest'].includes(key)) continue;
      if (typeof value === 'string' && /^(latest|stable|current|nightly)$/i.test(value)) {
        errors.push(`${group}.${key} must be an exact version, not ${value}.`);
      }
    }
  }
  for (const [name, value] of [
    ['rust.toolchain', model?.rust?.toolchain],
    ['dotnet.sdk', model?.dotnet?.sdk],
    ['dotnet.windows_app_sdk', model?.dotnet?.windows_app_sdk],
    ['node.node', model?.node?.node],
    ['node.npm', model?.node?.npm],
  ]) {
    if (!exactVersion(value)) errors.push(`${name} must be an exact semantic version.`);
  }
  for (const platform of ['apple', 'android', 'linux']) {
    if (model?.[platform]?.status !== 'interface-only') errors.push(`${platform}.status must be interface-only.`);
    if (model?.[platform]?.verification !== 'blocked_environment') errors.push(`${platform}.verification must be blocked_environment.`);
  }
  if (model?.upgrade_process?.cadence !== 'quarterly') errors.push('upgrade_process.cadence must be quarterly.');
  if (!Array.isArray(model?.upgrade_process?.required_steps) || model.upgrade_process.required_steps.length < 5) {
    errors.push('upgrade_process.required_steps must contain at least five concrete steps.');
  }
  if (!Array.isArray(model?.upgrade_process?.required_evidence) || model.upgrade_process.required_evidence.length < 4) {
    errors.push('upgrade_process.required_evidence must contain at least four evidence gates.');
  }
  return errors;
}

export function validateToolchainLock(rootDir, { probe = true } = {}) {
  const root = path.resolve(rootDir);
  const errors = [];
  const statuses = {};
  const model = loadJson(root, 'toolchains/toolchain.lock.json', errors);
  if (!model) return { errors, statuses };
  errors.push(...validateToolchainModel(model));

  if (!fs.existsSync(path.join(root, '.nvmrc'))) errors.push('Missing .nvmrc.');
  else if (read(root, '.nvmrc').trim() !== model.node.node) errors.push('.nvmrc does not match node.node.');

  if (!fs.existsSync(path.join(root, 'rust-toolchain.toml'))) errors.push('Missing rust-toolchain.toml.');
  else {
    const rust = read(root, 'rust-toolchain.toml');
    if (!rust.includes(`channel = "${model.rust.toolchain}"`)) errors.push('rust-toolchain.toml channel does not match rust.toolchain.');
    for (const component of model.rust.components) {
      if (!rust.includes(`"${component}"`)) errors.push(`rust-toolchain.toml is missing component ${component}.`);
    }
    if (!rust.includes(`"${model.rust.target}"`)) errors.push('rust-toolchain.toml target does not match rust.target.');
  }

  const global = loadJson(root, 'global.json', errors);
  if (global?.sdk?.version !== model.dotnet.sdk) errors.push('global.json SDK does not match dotnet.sdk.');

  const workflow = fs.existsSync(path.join(root, '.github/workflows/ci.yml')) ? read(root, '.github/workflows/ci.yml') : '';
  if (!workflow.includes('node-version-file: .nvmrc')) errors.push('CI must consume .nvmrc via node-version-file.');
  if (!workflow.includes(`toolchain: ${model.rust.toolchain}`)) errors.push('CI must consume the locked Rust toolchain.');
  if (!workflow.includes(`dotnet-version: ${model.dotnet.sdk}`)) errors.push('CI must consume the locked .NET SDK.');

  if (probe) {
    const probes = {
      rust: commandVersion('rustc', ['--version']),
      node: commandVersion('node', ['--version']),
      npm: commandVersion(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['--version']),
      dotnet: commandVersion('dotnet', ['--version']),
      swift: commandVersion('swift', ['--version']),
      xcodebuild: commandVersion('xcodebuild', ['-version']),
      gradle: commandVersion('gradle', ['--version']),
      meson: commandVersion('meson', ['--version']),
    };
    for (const [tool, result] of Object.entries(probes)) {
      if (result.status === 'blocked_environment') {
        statuses[tool] = 'blocked_environment';
        continue;
      }
      const actual = normalizeVersion(result.output, tool);
      statuses[tool] = { status: 'available', version: actual };
      const expected = tool === 'rust' ? model.rust.toolchain : tool === 'node' ? model.node.node : tool === 'npm' ? model.node.npm : model.dotnet.sdk;
      if (['rust', 'node', 'npm', 'dotnet'].includes(tool) && actual !== expected) {
        errors.push(`${tool} version ${actual} does not match locked version ${expected}.`);
      }
    }
  }
  return { errors, statuses };
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  const root = process.argv[2] ?? process.cwd();
  const result = validateToolchainLock(root, { probe: !process.argv.includes('--no-probe') });
  if (Object.keys(result.statuses).length > 0) console.log(`toolchain probe statuses: ${JSON.stringify(result.statuses)}`);
  if (result.errors.length > 0) {
    console.error(`Toolchain lock invalid with ${result.errors.length} error(s):`);
    for (const error of result.errors) console.error(`- ${error}`);
    process.exitCode = 1;
  } else {
    console.log('Toolchain lock valid: local/CI lock consumers and upgrade policy verified.');
  }
}
